// SPDX-License-Identifier: Apache-2.0
//! JT display-model record extractors and their record types.

use std::io::Read as _;

use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::{Tessellation, TessellationChannel};
use cadmpeg_ir::SourceObjectAssociation;

#[allow(clippy::wildcard_imports)]
use super::*;

/// Outer index of the embedded JT display-model stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtIndex {
    /// Globally unique index identity.
    pub id: String,
    /// Serialized index version.
    pub version: u32,
    /// Declared number of indexed JT documents.
    pub declared_count: u32,
    /// Indexed document rows in serialized order.
    pub rows: Vec<DisplayJtIndexRow>,
    /// Absolute source offset of the `DisplayJT` payload.
    pub source_offset: u64,
}

/// One physical-header offset and associated value in a `DisplayJT` index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based row index.
    pub ordinal: u32,
    /// Payload-relative physical JT-header offset.
    pub header_offset: u32,
    /// Nonzero serialized row value whose semantic role is unassigned.
    pub value: u64,
    /// Absolute source offset of the row.
    pub source_offset: u64,
}

/// One bounded embedded JT document and its table of contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtDocument {
    /// Globally unique document identity.
    pub id: String,
    /// Owning outer-index row.
    pub index_row: String,
    /// Exact 80-byte UTF-8 version field.
    pub version_field: String,
    /// JT format major version parsed from the version field.
    pub format_major: u16,
    /// JT format minor version parsed from the version field.
    pub format_minor: u16,
    /// Serialized JT byte-order flag.
    pub byte_order: u8,
    /// Payload-relative table-of-contents offset.
    pub toc_offset: u32,
    /// Exact 16-byte logical scene-graph segment identifier.
    pub lsg_segment_id: Vec<u8>,
    /// Ordered table-of-contents entries.
    pub toc_entries: Vec<DisplayJtTocEntry>,
    /// Physical byte length ending at the next indexed header or stream boundary.
    pub physical_byte_len: u64,
    /// Absolute source offset of the JT version field.
    pub source_offset: u64,
}

/// One fixed-width entry in an embedded JT document table of contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtTocEntry {
    /// Globally unique TOC-entry identity.
    pub id: String,
    /// Zero-based serialized entry order.
    pub ordinal: u32,
    /// Exact 16-byte segment identifier.
    pub segment_id: Vec<u8>,
    /// Document-relative segment offset.
    pub segment_offset: u32,
    /// Physical segment byte length.
    pub segment_byte_len: u32,
    /// Exact four-byte segment attribute field.
    pub attributes: Vec<u8>,
    /// Absolute source offset of the TOC entry.
    pub source_offset: u64,
}

/// One physically bounded segment in an embedded JT document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtSegment {
    /// Globally unique segment identity.
    pub id: String,
    /// Owning JT document.
    pub document: String,
    /// Owning table-of-contents entry.
    pub toc_entry: String,
    /// Exact 16-byte segment identifier.
    pub segment_id: Vec<u8>,
    /// Segment type repeated by the table-of-contents attribute word.
    pub segment_type: u32,
    /// Physical segment byte length, including its 24-byte header.
    pub segment_byte_len: u32,
    /// SHA-256 of the bytes following the segment header.
    pub payload_sha256: String,
    /// Complete compressed-data envelope when the payload is compressed.
    pub compression: Option<DisplayJtCompression>,
    /// Absolute source offset of the segment header.
    pub source_offset: u64,
}

/// Validated compressed-data envelope following a JT segment header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtCompression {
    /// Serialized compression flag.
    pub flag: u32,
    /// Declared byte length of the algorithm byte and compressed member.
    pub compressed_data_byte_len: u32,
    /// Serialized compression algorithm identifier.
    pub algorithm: u8,
    /// Physical zlib-member byte length.
    pub compressed_byte_len: u32,
    /// SHA-256 of the completely inflated payload.
    pub inflated_sha256: String,
}

/// One length-bounded object element in a JT shape-LOD segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtShapeLodElement {
    /// Globally unique element identity.
    pub id: String,
    /// Owning type-7 segment.
    pub segment: String,
    /// Zero-based serialized element order.
    pub ordinal: u32,
    /// Exact 16-byte object-type identifier.
    pub object_type_id: Vec<u8>,
    /// Serialized object-base-type discriminator.
    pub object_base_type: u8,
    /// Serialized object identifier.
    pub object_id: u32,
    /// Bytes following the common element header.
    pub body_byte_len: u32,
    /// SHA-256 of the bytes following the common element header.
    pub body_sha256: String,
    /// Absolute source offset of the element length.
    pub source_offset: u64,
}

/// Fixed version and binding header of a JT 9 tri-strip shape-LOD element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtTriStripLodHeader {
    /// Globally unique header identity.
    pub id: String,
    /// Owning shape-LOD element.
    pub element: String,
    /// Base shape-LOD data version.
    pub base_version: u16,
    /// Vertex shape-LOD data version.
    pub vertex_version: u16,
    /// Packed vertex-channel binding mask.
    pub vertex_bindings: u64,
    /// Topological mesh LOD data version.
    pub topological_mesh_version: u16,
    /// Serialized object identifier shared by the vertex records.
    pub vertex_records_object_id: u32,
    /// Compressed topological-mesh representation version.
    pub compressed_lod_version: u16,
    /// Bytes following the fixed header.
    pub compressed_representation_byte_len: u32,
    /// SHA-256 of the bytes following the fixed header.
    pub compressed_representation_sha256: String,
    /// Absolute source offset of the fixed header.
    pub source_offset: u64,
}

/// Decoded context-zero face-degree symbols from a JT topological mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtInitialFaceDegreeSymbols {
    /// Globally unique symbol-vector identity.
    pub id: String,
    /// Owning tri-strip shape-LOD element.
    pub element: String,
    /// Decoded symbols in topology-coder visit order.
    pub degrees: Vec<i32>,
    /// Complete compressed-packet byte length.
    pub packet_byte_len: u32,
    /// SHA-256 of the complete compressed packet.
    pub packet_sha256: String,
    /// Absolute source offset of the compressed packet.
    pub source_offset: u64,
}

/// One structurally bounded compressed topology vector in a JT 9 mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtTopologyPacket {
    /// Stable semantic lane name.
    pub role: String,
    /// Number of values represented by the packet.
    pub value_count: u32,
    /// Serialized compression codec identifier; zero denotes an empty vector.
    pub codec: u8,
    /// Complete packet length in bytes.
    pub byte_len: u32,
    /// Digest of the complete packet bytes.
    pub sha256: String,
    /// Mesh-representation-relative packet offset.
    pub representation_offset: u32,
    /// Reconstructed primal values when the packet codec is decoded.
    pub values: Option<Vec<i32>>,
}

/// Complete compressed-topology envelope preceding JT 9 vertex records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtTopologyPacketSequence {
    /// Globally unique sequence identity.
    pub id: String,
    /// Owning tri-strip shape-LOD element.
    pub element: String,
    /// Ordered topology vectors.
    pub packets: Vec<DisplayJtTopologyPacket>,
    /// Integrity hash serialized after the topology vectors.
    pub composite_hash: u32,
    /// Total topology-envelope length including the hash.
    pub topology_byte_len: u32,
    /// Absolute source offset of the compressed representation.
    pub source_offset: u64,
}

/// Polygon connectivity reconstructed from one JT topological dual mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtPolygonMesh {
    /// Globally unique polygon-mesh identity.
    pub id: String,
    /// Owning topology packet sequence.
    pub topology: String,
    /// Coordinate-array header indexed by the polygons.
    pub coordinate_header: String,
    /// Ordered polygon vertex indices.
    pub polygons: Vec<Vec<u32>>,
    /// Per-corner vertex-attribute indices parallel to `polygons`.
    pub vertex_attribute_indices: Vec<Vec<Option<u32>>>,
    /// Per-polygon group identifiers.
    pub polygon_groups: Vec<i32>,
    /// Per-polygon flag words.
    pub polygon_flags: Vec<u16>,
    /// Absolute source offset of the topology packet sequence.
    pub source_offset: u64,
}

/// Fixed header of the vertex records following a JT 9 topology envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtCompressedVertexRecordsHeader {
    /// Globally unique header identity.
    pub id: String,
    /// Owning tri-strip shape-LOD element.
    pub element: String,
    /// Packed vertex-channel binding mask.
    pub vertex_bindings: u64,
    /// Quantization bits per vertex coordinate component.
    pub vertex_quantization_bits: u8,
    /// Normal quantization factor.
    pub normal_quantization_factor: u8,
    /// Quantization bits per texture-coordinate component.
    pub texture_quantization_bits: u8,
    /// Quantization bits per color component.
    pub color_quantization_bits: u8,
    /// Number of unique topological vertices.
    pub topological_vertex_count: u32,
    /// Number of vertex-attribute records.
    pub vertex_attribute_count: u32,
    /// Remaining compressed vertex-array length.
    pub compressed_arrays_byte_len: u32,
    /// Digest of the remaining compressed vertex arrays.
    pub compressed_arrays_sha256: String,
    /// Absolute source offset of this header.
    pub source_offset: u64,
}

/// Fixed quantization envelope of a JT 9 compressed coordinate array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayJtVertexCoordinateArrayHeader {
    /// Globally unique header identity.
    pub id: String,
    /// Owning tri-strip shape-LOD element.
    pub element: String,
    /// Number of unique coordinate records.
    pub unique_vertex_count: u32,
    /// Number of coordinate components per record.
    pub component_count: u8,
    /// Inclusive component ranges as minimum and maximum pairs for X, Y, and Z.
    pub component_ranges: [[f32; 2]; 3],
    /// Quantization bits for X, Y, and Z.
    pub component_quantization_bits: [u8; 3],
    /// Remaining compressed component-data length.
    pub compressed_components_byte_len: u32,
    /// Digest of the remaining compressed component data.
    pub compressed_components_sha256: String,
    /// Absolute source offset of this header.
    pub source_offset: u64,
}

/// Model-space coordinates decoded from one JT 9 vertex array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayJtVertexCoordinates {
    /// Globally unique coordinate-array identity.
    pub id: String,
    /// Owning coordinate-array header.
    pub header: String,
    /// XYZ coordinates in the JT model's serialized metre unit.
    pub points_m: Vec<[f32; 3]>,
    /// Combined hash serialized after the component vectors.
    pub coordinate_hash: u32,
    /// Complete byte length of the component packets and hash.
    pub byte_len: u32,
    /// Absolute source offset of the first component packet.
    pub source_offset: u64,
}

/// Normal vectors decoded from one JT 9 vertex-attribute array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayJtVertexNormals {
    /// Globally unique normal-array identity.
    pub id: String,
    /// Owning compressed vertex-record header.
    pub vertex_records_header: String,
    /// Ordered unit normal vectors in attribute-record order.
    pub normals: Vec<[f32; 3]>,
    /// Combined hash serialized after the component vectors.
    pub normal_hash: u32,
    /// Complete byte length of the normal-array header, packets, and hash.
    pub byte_len: u32,
    /// Absolute source offset of the normal-array count.
    pub source_offset: u64,
}

/// Colors decoded from one JT 9 vertex-attribute array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayJtVertexColors {
    /// Globally unique color-array identity.
    pub id: String,
    /// Owning compressed vertex-record header.
    pub vertex_records_header: String,
    /// Ordered RGBA colors in vertex-attribute record order.
    pub colors: Vec<[f32; 4]>,
    /// Combined hash serialized after the component vectors.
    pub color_hash: u32,
    /// Complete byte length of the color-array header, packets, and hash.
    pub byte_len: u32,
    /// Absolute source offset of the color-array count.
    pub source_offset: u64,
}

/// One decoded JT 9 vertex texture-coordinate channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayJtVertexTextureCoordinates {
    /// Globally unique channel identity.
    pub id: String,
    /// Owning compressed vertex-record header.
    pub vertex_records_header: String,
    /// Zero-based texture-coordinate channel selected by the binding nibble.
    pub channel: u8,
    /// Ordered component vectors in vertex-attribute record order.
    pub values: Vec<Vec<f32>>,
    /// Combined hash serialized after the component vectors.
    pub texture_coordinate_hash: u32,
    /// Complete byte length of the array header, packets, and hash.
    pub byte_len: u32,
    /// Absolute source offset of the texture-coordinate count.
    pub source_offset: u64,
}

/// One decoded JT 9 vertex-flag array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtVertexFlags {
    /// Globally unique flag-array identity.
    pub id: String,
    /// Owning compressed vertex-record header.
    pub vertex_records_header: String,
    /// Ordered zero-or-one flag values in vertex-attribute record order.
    pub values: Vec<u32>,
    /// Complete byte length of the count and compressed packet.
    pub byte_len: u32,
    /// Absolute source offset of the vertex-flag count.
    pub source_offset: u64,
}

/// Complete JT 9 tri-strip shape node controlling one late-loaded mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayJtTriStripShapeNode {
    /// Globally unique shape-node identity.
    pub id: String,
    /// Owning common node-data record.
    pub base_node: String,
    /// Serialized node object identifier.
    pub object_id: u32,
    /// Reserved model-coordinate bounds.
    pub reserved_bounds: [[f32; 3]; 2],
    /// Untransformed model-coordinate bounds.
    pub untransformed_bounds: [[f32; 3]; 2],
    /// Surface area in normalized coordinate space.
    pub area: f32,
    /// Minimum and maximum vertex counts.
    pub vertex_count_range: [i32; 2],
    /// Minimum and maximum scene-node counts.
    pub node_count_range: [i32; 2],
    /// Minimum and maximum polygon counts.
    pub polygon_count_range: [i32; 2],
    /// Expected in-memory byte size of the late-loaded LOD.
    pub memory_byte_len: u32,
    /// Qualitative compression level in the inclusive range zero through one.
    pub compression_level: f32,
    /// Vertex-shape data version.
    pub vertex_version: u16,
    /// Packed vertex-channel binding mask.
    pub vertex_bindings: u64,
    /// Quantization bits per vertex coordinate component.
    pub vertex_quantization_bits: u8,
    /// Normal quantization factor.
    pub normal_quantization_factor: u8,
    /// Quantization bits per texture-coordinate component.
    pub texture_quantization_bits: u8,
    /// Quantization bits per color component.
    pub color_quantization_bits: u8,
    /// Version-2 repeated vertex-channel binding mask.
    pub version_2_vertex_bindings: Option<u64>,
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

/// One object element decoded from a compressed JT segment payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtCompressedElement {
    /// Globally unique element identity.
    pub id: String,
    /// Owning compressed segment.
    pub segment: String,
    /// Owning segment type.
    pub segment_type: u32,
    /// Zero-based serialized element order.
    pub ordinal: u32,
    /// Exact 16-byte object-type identifier.
    pub object_type_id: Vec<u8>,
    /// Serialized object-base-type discriminator.
    pub object_base_type: u8,
    /// Serialized object identifier.
    pub object_id: u32,
    /// Bytes following the common element header.
    pub body_byte_len: u32,
    /// SHA-256 of the bytes following the common element header.
    pub body_sha256: String,
    /// Offset of the element length in the inflated payload.
    pub inflated_offset: u32,
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

/// Complete element sequence and post-marker tail of one compressed JT segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtCompressedElementSequence {
    /// Globally unique sequence identity.
    pub id: String,
    /// Owning compressed segment.
    pub segment: String,
    /// Owning segment type.
    pub segment_type: u32,
    /// Ordered decoded element identities.
    pub elements: Vec<String>,
    /// Inflated byte length through the end-object marker.
    pub framed_byte_len: u32,
    /// Exact bytes following the end-object marker.
    pub tail: Vec<u8>,
    /// SHA-256 of the exact post-marker tail.
    pub tail_sha256: String,
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

/// One UTF-16 string property atom in a type-31 JT segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtStringPropertyAtom {
    /// Globally unique property-atom identity.
    pub id: String,
    /// Owning compressed element.
    pub element: String,
    /// Serialized object identifier.
    pub object_id: u32,
    /// Exact serialized UTF-16 code units.
    pub code_units: Vec<u16>,
    /// Decoded string value.
    pub value: String,
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

/// Property-table link from a logical shape node to a late-loaded LOD segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtShapeLodBinding {
    /// Globally unique binding identity.
    pub id: String,
    /// Owning type-1 logical scene-graph segment.
    pub scene_segment: String,
    /// Serialized property-table version.
    pub table_version: u16,
    /// Shape-node object identifier owning the property pair.
    pub shape_node_object_id: u32,
    /// String-property object identifier used as the key.
    pub key_object_id: u32,
    /// Exact decoded property key.
    pub key: String,
    /// Late-loaded-property object identifier used as the value.
    pub value_object_id: u32,
    /// Base-property state flags.
    pub state_flags: u32,
    /// Late-loaded-property version.
    pub property_version: u16,
    /// Resolved type-7 shape-LOD segment.
    pub shape_segment: String,
    /// Serialized payload object identifier within the shape-LOD segment.
    pub payload_object_id: u32,
    /// Serialized positive late-loaded-property reserved value.
    pub reserved_value: u32,
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

/// Common node-data header carried by one type-1 JT element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtBaseNodeData {
    /// Globally unique node-data identity.
    pub id: String,
    /// Owning compressed element.
    pub element: String,
    /// Exact 16-byte object-type identifier of the owning element.
    pub object_type_id: Vec<u8>,
    /// Serialized node object identifier.
    pub object_id: u32,
    /// Common node-data version.
    pub version: u16,
    /// Serialized node flags.
    pub flags: u32,
    /// Ordered attribute object identifiers.
    pub attribute_object_ids: Vec<u32>,
    /// Byte length after the common node-data header.
    pub family_data_byte_len: u32,
    /// SHA-256 of the bytes after the common node-data header.
    pub family_data_sha256: String,
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

/// Complete JT 9 instance node referencing one shared logical scene node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtInstanceNode {
    /// Globally unique instance-node identity.
    pub id: String,
    /// Owning common node-data record.
    pub base_node: String,
    /// Serialized instance-node object identifier.
    pub object_id: u32,
    /// Instance-node data version.
    pub version: u16,
    /// Referenced child node object identifier.
    pub child_object_id: u32,
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

/// Common JT 9 group-node data carried by every group-derived scene node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayJtGroupNodeData {
    /// Globally unique group-data identity.
    pub id: String,
    /// Owning common node-data record.
    pub base_node: String,
    /// Serialized group-derived node object identifier.
    pub object_id: u32,
    /// Group-node data version.
    pub version: u16,
    /// Ordered child node object identifiers.
    pub child_object_ids: Vec<u32>,
    /// Byte length after the common group-node data.
    pub family_data_byte_len: u32,
    /// SHA-256 of the bytes after the common group-node data.
    pub family_data_sha256: String,
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

/// One JT geometric-transform attribute attached to logical scene nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayJtGeometricTransformAttribute {
    /// Globally unique transform-attribute identity.
    pub id: String,
    /// Owning compressed logical scene-graph element.
    pub element: String,
    /// Serialized attribute object identifier referenced by nodes.
    pub object_id: u32,
    /// Base-attribute state flags.
    pub state_flags: u8,
    /// Base-attribute field-inhibit flags.
    pub field_inhibit_flags: u32,
    /// Sparse-matrix stored-values mask in row-major bit order.
    pub stored_values_mask: u16,
    /// Complete row-major local-to-parent homogeneous matrix.
    pub matrix: [[f32; 4]; 4],
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

/// Complete JT 9 partition node linking an LSG branch to a partition file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayJtPartitionNode {
    /// Globally unique partition-node identity.
    pub id: String,
    /// Owning common node-data record.
    pub base_node: String,
    /// Serialized node object identifier.
    pub object_id: u32,
    /// Group-node data version.
    pub group_version: u16,
    /// Ordered child node object identifiers.
    pub child_object_ids: Vec<u32>,
    /// Serialized partition flags.
    pub partition_flags: u32,
    /// Exact partition filename UTF-16 code units.
    pub file_name_code_units: Vec<u16>,
    /// Decoded partition filename.
    pub file_name: String,
    /// Transformed axis-aligned bounds as minimum and maximum XYZ corners.
    pub transformed_bounds: [[f32; 3]; 2],
    /// Total descendant surface area in normalized coordinate space.
    pub area: f32,
    /// Minimum and maximum descendant vertex counts.
    pub vertex_count_range: [i32; 2],
    /// Minimum and maximum descendant node counts.
    pub node_count_range: [i32; 2],
    /// Minimum and maximum descendant polygon counts.
    pub polygon_count_range: [i32; 2],
    /// Untransformed bounds when partition flag bit zero is set.
    pub untransformed_bounds: Option<[[f32; 3]; 2]>,
    /// Reserved bounds when partition flag bit zero is clear.
    pub reserved_bounds: Option<[[f32; 3]; 2]>,
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

/// Complete JT 9 range-LOD node selecting among ordered child nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayJtRangeLodNode {
    /// Globally unique range-LOD-node identity.
    pub id: String,
    /// Owning common node-data record.
    pub base_node: String,
    /// Serialized node object identifier.
    pub object_id: u32,
    /// Group-node data version.
    pub group_version: u16,
    /// Ordered alternate-representation child identifiers.
    pub child_object_ids: Vec<u32>,
    /// LOD-node data version.
    pub lod_version: u16,
    /// Reserved finite floating-point vector.
    pub reserved_values: Vec<f32>,
    /// Reserved signed integer.
    pub reserved_value: i32,
    /// Range-LOD data version.
    pub range_version: u16,
    /// Strictly increasing nonnegative eye-distance limits.
    pub range_limits: Vec<f32>,
    /// Model-coordinate centre for range selection.
    pub center: [f32; 3],
    /// Absolute source offset of the owning compressed envelope.
    pub source_offset: u64,
}

struct ParsedJtElement<'a> {
    offset: usize,
    object_type_id: &'a [u8],
    object_id: u32,
    object_base_type: u8,
    body: &'a [u8],
}

fn parse_jt_element_sequence(payload: &[u8]) -> Option<(Vec<ParsedJtElement<'_>>, usize)> {
    const END_OBJECT_TYPE: [u8; 16] = [0xff; 16];
    let mut elements = Vec::new();
    let mut cursor = 0usize;
    loop {
        let element_byte_len = payload
            .get(cursor..cursor.checked_add(4)?)
            .and_then(|value| value.try_into().ok())
            .map(u32::from_le_bytes)?;
        let element_end = cursor
            .checked_add(4)?
            .checked_add(usize::try_from(element_byte_len).ok()?)?;
        let element = payload.get(cursor + 4..element_end)?;
        if element_byte_len == 16 && element == END_OBJECT_TYPE {
            return Some((elements, element_end));
        }
        let object_type_id = element.get(..16)?;
        let &object_base_type = element.get(16)?;
        let object_id = element
            .get(17..21)
            .and_then(|value| value.try_into().ok())
            .map(u32::from_le_bytes)?;
        elements.push(ParsedJtElement {
            offset: cursor,
            object_type_id,
            object_id,
            object_base_type,
            body: &element[21..],
        });
        cursor = element_end;
    }
}

pub(crate) fn parse_jt_string_property_atom_body(body: &[u8]) -> Option<(Vec<u16>, String)> {
    const PREFIX: [u8; 8] = [1, 0, 0, 0, 0, 0x40, 1, 0];
    if body.get(..8) != Some(PREFIX.as_slice()) {
        return None;
    }
    let count = body
        .get(8..12)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)?;
    let count = usize::try_from(count).ok()?;
    if body.len() != 12usize.checked_add(count.checked_mul(2)?)? {
        return None;
    }
    let code_units = body[12..]
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
        .collect::<Vec<_>>();
    let value = String::from_utf16(&code_units).ok()?;
    Some((code_units, value))
}

pub(crate) fn parse_jt9_tri_strip_lod_header(body: &[u8]) -> Option<(u64, u16, u32, u16, &[u8])> {
    if body.len() < 20 {
        return None;
    }
    let base_version = u16::from_le_bytes(body[0..2].try_into().ok()?);
    let vertex_version = u16::from_le_bytes(body[2..4].try_into().ok()?);
    let vertex_bindings = u64::from_le_bytes(body[4..12].try_into().ok()?);
    let topological_mesh_version = u16::from_le_bytes(body[12..14].try_into().ok()?);
    let vertex_records_object_id = u32::from_le_bytes(body[14..18].try_into().ok()?);
    let compressed_lod_version = u16::from_le_bytes(body[18..20].try_into().ok()?);
    if base_version != 1 || vertex_version != 1 || !matches!(topological_mesh_version, 1 | 2) {
        return None;
    }
    if !matches!(compressed_lod_version, 1 | 2) {
        return None;
    }
    Some((
        vertex_bindings,
        topological_mesh_version,
        vertex_records_object_id,
        compressed_lod_version,
        &body[20..],
    ))
}

pub(crate) fn jt9_topology_high_degree_lane_count(
    representation: &[u8],
    expected_vertex_bindings: u64,
) -> Option<usize> {
    const PREFIX_PACKET_COUNT: usize = 21;
    let mut prefix_end = 0usize;
    for _ in 0..PREFIX_PACKET_COUNT {
        let (_, _, byte_len) = crate::jt::frame_int32_cdp2(representation.get(prefix_end..)?, 0)?;
        prefix_end = prefix_end.checked_add(byte_len)?;
    }
    let mut match_count = 0usize;
    let mut matched_lane_count = 0usize;
    let mut cursor = prefix_end;
    for lane_count in 1..=64 {
        let Some((_, _, byte_len)) = representation
            .get(cursor..)
            .and_then(|bytes| crate::jt::frame_int32_cdp2(bytes, 0))
        else {
            break;
        };
        cursor = cursor.checked_add(byte_len)?;
        let mut candidate_end = cursor;
        let mut split_packets_valid = true;
        for _ in 0..2 {
            let Some((_, _, byte_len)) = representation
                .get(candidate_end..)
                .and_then(|bytes| crate::jt::frame_int32_cdp2(bytes, 0))
            else {
                split_packets_valid = false;
                break;
            };
            candidate_end = candidate_end.checked_add(byte_len)?;
        }
        if !split_packets_valid {
            continue;
        }
        let Some(header_end) = candidate_end.checked_add(20) else {
            continue;
        };
        let Some(envelope) = representation.get(candidate_end..header_end) else {
            continue;
        };
        let bindings = u64::from_le_bytes(envelope[4..12].try_into().ok()?);
        let quantization = &envelope[12..16];
        let topological_vertex_count = u32::from_le_bytes(envelope[16..20].try_into().ok()?);
        let vertex_attribute_count = if topological_vertex_count == 0 {
            0
        } else {
            let attribute_end = header_end.checked_add(4)?;
            let Some(bytes) = representation.get(header_end..attribute_end) else {
                continue;
            };
            u32::from_le_bytes(bytes.try_into().ok()?)
        };
        if bindings == expected_vertex_bindings
            && quantization[0] <= 24
            && quantization[1] <= 13
            && quantization[2] <= 24
            && quantization[3] <= 24
            && i32::try_from(topological_vertex_count).is_ok()
            && i32::try_from(vertex_attribute_count).is_ok()
        {
            match_count += 1;
            matched_lane_count = lane_count;
        }
    }
    (match_count == 1).then_some(matched_lane_count)
}

pub(crate) fn parse_jt_base_node_body(
    body: &[u8],
    format_major: u16,
) -> Option<(u16, u32, Vec<u32>, &[u8])> {
    let (version, flags_offset, count_offset, attributes_offset): (u16, usize, usize, usize) =
        if format_major < 10 {
            (
                u16::from_le_bytes(body.get(..2)?.try_into().ok()?),
                2,
                6,
                10,
            )
        } else {
            (u16::from(*body.first()?), 1, 5, 9)
        };
    let flags = u32::from_le_bytes(body.get(flags_offset..flags_offset + 4)?.try_into().ok()?);
    let attribute_count =
        u32::from_le_bytes(body.get(count_offset..count_offset + 4)?.try_into().ok()?);
    let attribute_count = usize::try_from(attribute_count).ok()?;
    let header_end = attributes_offset.checked_add(attribute_count.checked_mul(4)?)?;
    let attributes = body.get(attributes_offset..header_end)?;
    let attribute_object_ids = attributes
        .chunks_exact(4)
        .map(|value| u32::from_le_bytes(value.try_into().expect("four-byte chunk")))
        .collect();
    Some((version, flags, attribute_object_ids, &body[header_end..]))
}

pub(crate) fn parse_jt9_instance_node_body(body: &[u8]) -> Option<(u16, u32)> {
    let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
    let version = u16::from_le_bytes(family.get(..2)?.try_into().ok()?);
    let child_object_id = u32::from_le_bytes(family.get(2..6)?.try_into().ok()?);
    (version == 1 && family.len() == 6).then_some((version, child_object_id))
}

pub(crate) struct ParsedJtTriStripShapeNode {
    pub(crate) reserved_bounds: [[f32; 3]; 2],
    pub(crate) untransformed_bounds: [[f32; 3]; 2],
    pub(crate) area: f32,
    pub(crate) vertex_count_range: [i32; 2],
    pub(crate) node_count_range: [i32; 2],
    pub(crate) polygon_count_range: [i32; 2],
    pub(crate) memory_byte_len: u32,
    pub(crate) compression_level: f32,
    pub(crate) vertex_version: u16,
    pub(crate) vertex_bindings: u64,
    pub(crate) vertex_quantization_bits: u8,
    pub(crate) normal_quantization_factor: u8,
    pub(crate) texture_quantization_bits: u8,
    pub(crate) color_quantization_bits: u8,
    pub(crate) version_2_vertex_bindings: Option<u64>,
}

pub(crate) fn parse_jt9_tri_strip_shape_node_body(
    body: &[u8],
) -> Option<ParsedJtTriStripShapeNode> {
    let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
    if family.len() < 100 || u16::from_le_bytes(family[..2].try_into().ok()?) != 1 {
        return None;
    }
    let f32_at = |offset: usize| {
        family
            .get(offset..offset + 4)
            .and_then(|value| value.try_into().ok())
            .map(f32::from_le_bytes)
            .filter(|value| value.is_finite())
    };
    let bounds_at = |offset: usize| {
        let bounds = [
            [f32_at(offset)?, f32_at(offset + 4)?, f32_at(offset + 8)?],
            [
                f32_at(offset + 12)?,
                f32_at(offset + 16)?,
                f32_at(offset + 20)?,
            ],
        ];
        bounds[0]
            .iter()
            .zip(bounds[1])
            .all(|(minimum, maximum)| minimum <= &maximum)
            .then_some(bounds)
    };
    let range_at = |offset: usize| {
        let range = [
            i32::from_le_bytes(family.get(offset..offset + 4)?.try_into().ok()?),
            i32::from_le_bytes(family.get(offset + 4..offset + 8)?.try_into().ok()?),
        ];
        (range[0] >= 0 && range[0] <= range[1]).then_some(range)
    };
    let compression_level = f32_at(82).filter(|value| (0.0..=1.0).contains(value))?;
    let area = f32_at(50).filter(|value| *value >= 0.0)?;
    let vertex_version = u16::from_le_bytes(family[86..88].try_into().ok()?);
    if !matches!(vertex_version, 1 | 2) {
        return None;
    }
    let expected_len = if vertex_version == 1 { 100 } else { 108 };
    if family.len() != expected_len {
        return None;
    }
    let vertex_bindings = u64::from_le_bytes(family[88..96].try_into().ok()?);
    let vertex_quantization_bits = family[96];
    let normal_quantization_factor = family[97];
    let texture_quantization_bits = family[98];
    let color_quantization_bits = family[99];
    let version_2_vertex_bindings = (vertex_version == 2).then(|| {
        u64::from_le_bytes(
            family[100..108]
                .try_into()
                .expect("version-2 binding lane is fixed-width"),
        )
    });
    if vertex_quantization_bits > 24
        || normal_quantization_factor > 13
        || texture_quantization_bits > 24
        || color_quantization_bits > 24
    {
        return None;
    }
    Some(ParsedJtTriStripShapeNode {
        reserved_bounds: bounds_at(2)?,
        untransformed_bounds: bounds_at(26)?,
        area,
        vertex_count_range: range_at(54)?,
        node_count_range: range_at(62)?,
        polygon_count_range: range_at(70)?,
        memory_byte_len: u32::from_le_bytes(family[78..82].try_into().ok()?),
        compression_level,
        vertex_version,
        vertex_bindings,
        vertex_quantization_bits,
        normal_quantization_factor,
        texture_quantization_bits,
        color_quantization_bits,
        version_2_vertex_bindings,
    })
}

pub(crate) struct ParsedJtPartitionNode {
    pub(crate) group_version: u16,
    pub(crate) child_object_ids: Vec<u32>,
    pub(crate) partition_flags: u32,
    pub(crate) file_name_code_units: Vec<u16>,
    pub(crate) file_name: String,
    pub(crate) transformed_bounds: [[f32; 3]; 2],
    pub(crate) area: f32,
    pub(crate) vertex_count_range: [i32; 2],
    pub(crate) node_count_range: [i32; 2],
    pub(crate) polygon_count_range: [i32; 2],
    pub(crate) untransformed_bounds: Option<[[f32; 3]; 2]>,
    pub(crate) reserved_bounds: Option<[[f32; 3]; 2]>,
}

fn parse_jt9_group_data(bytes: &[u8]) -> Option<(u16, Vec<u32>, &[u8])> {
    let version = u16::from_le_bytes(bytes.get(..2)?.try_into().ok()?);
    let count = u32::from_le_bytes(bytes.get(2..6)?.try_into().ok()?);
    let count = usize::try_from(count).ok()?;
    let end = 6usize.checked_add(count.checked_mul(4)?)?;
    let children = bytes
        .get(6..end)?
        .chunks_exact(4)
        .map(|value| u32::from_le_bytes(value.try_into().expect("four-byte chunk")))
        .collect();
    Some((version, children, &bytes[end..]))
}

pub(crate) fn parse_jt9_group_node_body(body: &[u8]) -> Option<(u16, Vec<u32>, &[u8])> {
    let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
    parse_jt9_group_data(family)
}

pub(crate) fn parse_jt9_partition_node_body(body: &[u8]) -> Option<ParsedJtPartitionNode> {
    let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
    let (group_version, child_object_ids, family) = parse_jt9_group_data(family)?;
    let partition_flags = u32::from_le_bytes(family.get(..4)?.try_into().ok()?);
    if partition_flags & !1 != 0 {
        return None;
    }
    let name_count_offset = 4usize;
    let name_count = u32::from_le_bytes(
        family
            .get(name_count_offset..name_count_offset.checked_add(4)?)?
            .try_into()
            .ok()?,
    );
    let name_count = usize::try_from(name_count).ok()?;
    let name_start = name_count_offset.checked_add(4)?;
    let name_end = name_start.checked_add(name_count.checked_mul(2)?)?;
    let file_name_code_units = family
        .get(name_start..name_end)?
        .chunks_exact(2)
        .map(|value| u16::from_le_bytes(value.try_into().expect("two-byte chunk")))
        .collect::<Vec<_>>();
    let file_name = String::from_utf16(&file_name_code_units).ok()?;
    if file_name.is_empty() || file_name.chars().any(char::is_control) {
        return None;
    }
    let f32_at = |offset: usize| {
        family
            .get(offset..offset.checked_add(4)?)
            .and_then(|value| value.try_into().ok())
            .map(f32::from_le_bytes)
            .filter(|value| value.is_finite())
    };
    let bounds_at = |offset: usize| {
        let bounds = [
            [f32_at(offset)?, f32_at(offset + 4)?, f32_at(offset + 8)?],
            [
                f32_at(offset + 12)?,
                f32_at(offset + 16)?,
                f32_at(offset + 20)?,
            ],
        ];
        bounds[0]
            .iter()
            .zip(bounds[1])
            .all(|(minimum, maximum)| *minimum <= maximum)
            .then_some(bounds)
    };
    let first_bounds = bounds_at(name_end)?;
    let mut cursor = name_end.checked_add(24)?;
    let (reserved_bounds, transformed_bounds) = if partition_flags & 1 == 0 {
        let transformed = bounds_at(cursor)?;
        cursor = cursor.checked_add(24)?;
        (Some(first_bounds), transformed)
    } else {
        (None, first_bounds)
    };
    let area = f32_at(cursor)?;
    if area < 0.0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let count_range = |offset: usize| {
        let minimum = i32::from_le_bytes(
            family
                .get(offset..offset.checked_add(4)?)?
                .try_into()
                .ok()?,
        );
        let maximum = i32::from_le_bytes(
            family
                .get(offset + 4..offset.checked_add(8)?)?
                .try_into()
                .ok()?,
        );
        (minimum >= 0 && (maximum == -1 || maximum >= minimum)).then_some([minimum, maximum])
    };
    let vertex_count_range = count_range(cursor)?;
    let node_count_range = count_range(cursor + 8)?;
    let polygon_count_range = count_range(cursor + 16)?;
    cursor = cursor.checked_add(24)?;
    let untransformed_bounds = if partition_flags & 1 != 0 {
        let bounds = bounds_at(cursor)?;
        cursor = cursor.checked_add(24)?;
        Some(bounds)
    } else {
        None
    };
    (cursor == family.len()).then_some(ParsedJtPartitionNode {
        group_version,
        child_object_ids,
        partition_flags,
        file_name_code_units,
        file_name,
        transformed_bounds,
        area,
        vertex_count_range,
        node_count_range,
        polygon_count_range,
        untransformed_bounds,
        reserved_bounds,
    })
}

pub(crate) struct ParsedJtRangeLodNode {
    pub(crate) group_version: u16,
    pub(crate) child_object_ids: Vec<u32>,
    pub(crate) lod_version: u16,
    pub(crate) reserved_values: Vec<f32>,
    pub(crate) reserved_value: i32,
    pub(crate) range_version: u16,
    pub(crate) range_limits: Vec<f32>,
    pub(crate) center: [f32; 3],
}

fn parse_jt_f32_vector(bytes: &[u8]) -> Option<(Vec<f32>, &[u8])> {
    let count = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
    let count = usize::try_from(count).ok()?;
    let end = 4usize.checked_add(count.checked_mul(4)?)?;
    let values = bytes
        .get(4..end)?
        .chunks_exact(4)
        .map(|value| f32::from_le_bytes(value.try_into().expect("four-byte chunk")))
        .collect::<Vec<_>>();
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some((values, &bytes[end..]))
}

pub(crate) fn parse_jt9_range_lod_node_body(body: &[u8]) -> Option<ParsedJtRangeLodNode> {
    let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
    let (group_version, child_object_ids, mut family) = parse_jt9_group_data(family)?;
    let lod_version = u16::from_le_bytes(family.get(..2)?.try_into().ok()?);
    family = &family[2..];
    let (reserved_values, remaining) = parse_jt_f32_vector(family)?;
    family = remaining;
    let reserved_value = i32::from_le_bytes(family.get(..4)?.try_into().ok()?);
    let range_version = u16::from_le_bytes(family.get(4..6)?.try_into().ok()?);
    let (range_limits, remaining) = parse_jt_f32_vector(&family[6..])?;
    if range_limits.iter().any(|value| *value < 0.0)
        || range_limits.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return None;
    }
    let center = [
        f32::from_le_bytes(remaining.get(0..4)?.try_into().ok()?),
        f32::from_le_bytes(remaining.get(4..8)?.try_into().ok()?),
        f32::from_le_bytes(remaining.get(8..12)?.try_into().ok()?),
    ];
    if remaining.len() != 12 || center.iter().any(|value| !value.is_finite()) {
        return None;
    }
    Some(ParsedJtRangeLodNode {
        group_version,
        child_object_ids,
        lod_version,
        reserved_values,
        reserved_value,
        range_version,
        range_limits,
        center,
    })
}

pub(crate) fn parse_jt9_geometric_transform_body(
    body: &[u8],
) -> Option<(u8, u32, u16, [[f32; 4]; 4])> {
    let base_version = u16::from_le_bytes(body.get(0..2)?.try_into().ok()?);
    let state_flags = *body.get(2)?;
    let field_inhibit_flags = u32::from_le_bytes(body.get(3..7)?.try_into().ok()?);
    let version = u16::from_le_bytes(body.get(7..9)?.try_into().ok()?);
    let stored_values_mask = u16::from_le_bytes(body.get(9..11)?.try_into().ok()?);
    if base_version != 1 || version != 1 || state_flags & !0x0f != 0 || field_inhibit_flags != 0 {
        return None;
    }
    let mut matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut cursor = 11usize;
    for index in 0..16 {
        if stored_values_mask & (0x8000 >> index) == 0 {
            continue;
        }
        let end = cursor.checked_add(4)?;
        let value = f32::from_le_bytes(body.get(cursor..end)?.try_into().ok()?);
        if !value.is_finite() {
            return None;
        }
        matrix[index / 4][index % 4] = value;
        cursor = cursor.checked_add(4)?;
    }
    if cursor != body.len()
        || matrix[0][3] != 0.0
        || matrix[1][3] != 0.0
        || matrix[2][3] != 0.0
        || matrix[3][3] != 1.0
    {
        return None;
    }
    let rows = [&matrix[0][..3], &matrix[1][..3], &matrix[2][..3]];
    let lengths = rows.map(|row| row.iter().map(|value| value * value).sum::<f32>().sqrt());
    if lengths
        .iter()
        .any(|length| !length.is_finite() || *length == 0.0)
    {
        return None;
    }
    for first in 0..3 {
        for second in first + 1..3 {
            let dot = rows[first]
                .iter()
                .zip(rows[second])
                .map(|(left, right)| left * right)
                .sum::<f32>();
            if dot.abs() > 1.0e-5 * lengths[first] * lengths[second] {
                return None;
            }
        }
    }
    Some((state_flags, field_inhibit_flags, stored_values_mask, matrix))
}

/// Decode the complete outer index of each `/Root/UG_PART/DisplayJT` stream.
pub fn display_jt_indices(container: &Container) -> Vec<DisplayJtIndex> {
    const JT_HEADER: &[u8] = b"Version ";
    let word_swapped_u64 = |bytes: &[u8]| -> Option<u64> {
        let high = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?);
        let low = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
        Some((u64::from(high) << 32) | u64::from(low))
    };
    container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/UG_PART/DisplayJT")
        .enumerate()
        .filter_map(|(index_ordinal, entry)| {
            let (source_offset, byte_len) = entry.file_span?;
            let start = usize::try_from(source_offset).ok()?;
            let byte_len = usize::try_from(byte_len).ok()?;
            let payload = container.data.get(start..start.checked_add(byte_len)?)?;
            let version = u32::from_le_bytes(payload.get(0..4)?.try_into().ok()?);
            let declared_count = u32::from_le_bytes(payload.get(4..8)?.try_into().ok()?);
            let row_count = usize::try_from(declared_count).ok()?;
            (row_count > 0).then_some(())?;
            let table_end = 8usize.checked_add(row_count.checked_mul(16)?)?;
            (table_end <= payload.len()).then_some(())?;
            let mut rows = Vec::with_capacity(row_count);
            let mut previous_header_offset = None;
            for ordinal in 0..row_count {
                let row_offset = 8 + ordinal * 16;
                let value = word_swapped_u64(payload.get(row_offset..row_offset + 8)?)?;
                let header_offset =
                    word_swapped_u64(payload.get(row_offset + 8..row_offset + 16)?)?;
                if value == 0 || header_offset > u64::from(u32::MAX) {
                    return None;
                }
                let header_offset_usize = usize::try_from(header_offset).ok()?;
                if header_offset_usize < table_end
                    || !payload
                        .get(header_offset_usize..)
                        .is_some_and(|tail| tail.starts_with(JT_HEADER))
                    || previous_header_offset.is_some_and(|previous| header_offset <= previous)
                {
                    return None;
                }
                previous_header_offset = Some(header_offset);
                rows.push(DisplayJtIndexRow {
                    id: format!("nx:display-jt:index#{index_ordinal}-row-{ordinal}"),
                    ordinal: ordinal as u32,
                    header_offset: header_offset as u32,
                    value,
                    source_offset: source_offset + row_offset as u64,
                });
            }
            Some(DisplayJtIndex {
                id: format!("nx:display-jt:index#{index_ordinal}"),
                version,
                declared_count,
                rows,
                source_offset,
            })
        })
        .collect()
}

/// Decode complete standard JT headers and tables of contents from an outer index.
pub fn display_jt_documents(
    container: &Container,
    indices: &[DisplayJtIndex],
) -> Vec<DisplayJtDocument> {
    const VERSION_FIELD_LEN: usize = 80;
    let entries = container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/UG_PART/DisplayJT")
        .collect::<Vec<_>>();
    let [entry] = entries.as_slice() else {
        return Vec::new();
    };
    let Some((stream_source_offset, stream_byte_len)) = entry.file_span else {
        return Vec::new();
    };
    let (Ok(stream_start), Ok(stream_byte_len)) = (
        usize::try_from(stream_source_offset),
        usize::try_from(stream_byte_len),
    ) else {
        return Vec::new();
    };
    let Some(stream) = container
        .data
        .get(stream_start..stream_start.saturating_add(stream_byte_len))
    else {
        return Vec::new();
    };
    let [index] = indices else {
        return Vec::new();
    };
    let mut documents = Vec::new();
    for (row_ordinal, row) in index.rows.iter().enumerate() {
        let Ok(document_start) = usize::try_from(row.header_offset) else {
            return Vec::new();
        };
        let document_end = index
            .rows
            .get(row_ordinal + 1)
            .map_or(stream.len(), |next| next.header_offset as usize);
        let Some(document) = stream.get(document_start..document_end) else {
            return Vec::new();
        };
        let Some(version_bytes) = document.get(..VERSION_FIELD_LEN) else {
            return Vec::new();
        };
        if !version_bytes.starts_with(b"Version ")
            || !version_bytes
                .iter()
                .all(|byte| byte.is_ascii_graphic() || byte.is_ascii_whitespace())
        {
            return Vec::new();
        }
        let Some(version_field) = std::str::from_utf8(version_bytes).ok() else {
            return Vec::new();
        };
        let Some(version_token) = version_field
            .strip_prefix("Version ")
            .and_then(|value| value.split_ascii_whitespace().next())
        else {
            return Vec::new();
        };
        let Some((format_major, format_minor)) = version_token.split_once('.') else {
            return Vec::new();
        };
        let (Ok(format_major), Ok(format_minor)) =
            (format_major.parse::<u16>(), format_minor.parse::<u16>())
        else {
            return Vec::new();
        };
        let Some(&byte_order) = document.get(80) else {
            return Vec::new();
        };
        if byte_order != 0 || document.get(81..85) != Some(&[0; 4]) {
            return Vec::new();
        }
        let Some(toc_offset) = document
            .get(85..89)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
        else {
            return Vec::new();
        };
        let Some(lsg_segment_id) = document.get(89..105) else {
            return Vec::new();
        };
        let Ok(toc_start) = usize::try_from(toc_offset) else {
            return Vec::new();
        };
        let Some(toc_count) = document
            .get(toc_start..toc_start.saturating_add(4))
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
        else {
            return Vec::new();
        };
        let Ok(toc_count_usize) = usize::try_from(toc_count) else {
            return Vec::new();
        };
        if toc_count_usize == 0 {
            return Vec::new();
        }
        let Some(toc_end) = toc_start
            .checked_add(4)
            .and_then(|start| start.checked_add(toc_count_usize.checked_mul(28)?))
        else {
            return Vec::new();
        };
        if toc_end > document.len() {
            return Vec::new();
        }
        let document_key = row
            .id
            .rsplit_once('#')
            .map_or(row.id.as_str(), |(_, key)| key);
        let mut toc_entries = Vec::with_capacity(toc_count_usize);
        for ordinal in 0..toc_count_usize {
            let offset = toc_start + 4 + ordinal * 28;
            let bytes = &document[offset..offset + 28];
            let segment_offset = u32::from_le_bytes(bytes[16..20].try_into().expect("fixed row"));
            let segment_byte_len = u32::from_le_bytes(bytes[20..24].try_into().expect("fixed row"));
            let Some(segment_end) = usize::try_from(segment_offset)
                .ok()
                .and_then(|start| start.checked_add(segment_byte_len as usize))
            else {
                return Vec::new();
            };
            if segment_byte_len == 0
                || (segment_offset as usize) < toc_end
                || segment_end > document.len()
            {
                return Vec::new();
            }
            toc_entries.push(DisplayJtTocEntry {
                id: format!("nx:display-jt:toc-entry#{document_key}-{ordinal}"),
                ordinal: ordinal as u32,
                segment_id: bytes[..16].to_vec(),
                segment_offset,
                segment_byte_len,
                attributes: bytes[24..28].to_vec(),
                source_offset: stream_source_offset + document_start as u64 + offset as u64,
            });
        }
        documents.push(DisplayJtDocument {
            id: format!("nx:display-jt:document#{document_key}"),
            index_row: row.id.clone(),
            version_field: version_field.to_string(),
            format_major,
            format_minor,
            byte_order,
            toc_offset,
            lsg_segment_id: lsg_segment_id.to_vec(),
            toc_entries,
            physical_byte_len: document.len() as u64,
            source_offset: stream_source_offset + document_start as u64,
        });
    }
    documents
}

/// Decode every segment declared by complete embedded JT documents.
pub fn display_jt_segments(
    container: &Container,
    documents: &[DisplayJtDocument],
) -> Vec<DisplayJtSegment> {
    let mut segments = Vec::new();
    for document in documents {
        let document_key = document
            .id
            .split_once('#')
            .map_or(document.id.as_str(), |(_, key)| key);
        let (Ok(start), Ok(byte_len)) = (
            usize::try_from(document.source_offset),
            usize::try_from(document.physical_byte_len),
        ) else {
            return Vec::new();
        };
        let Some(bytes) = container.data.get(start..start.saturating_add(byte_len)) else {
            return Vec::new();
        };
        for entry in &document.toc_entries {
            let (Ok(segment_start), Ok(segment_len)) = (
                usize::try_from(entry.segment_offset),
                usize::try_from(entry.segment_byte_len),
            ) else {
                return Vec::new();
            };
            let Some(segment) = bytes.get(segment_start..segment_start.saturating_add(segment_len))
            else {
                return Vec::new();
            };
            let Some(segment_id) = segment.get(..16) else {
                return Vec::new();
            };
            let Some(segment_type) = segment
                .get(16..20)
                .and_then(|value| value.try_into().ok())
                .map(u32::from_le_bytes)
            else {
                return Vec::new();
            };
            let Some(header_byte_len) = segment
                .get(20..24)
                .and_then(|value| value.try_into().ok())
                .map(u32::from_le_bytes)
            else {
                return Vec::new();
            };
            let Some(attribute_type) = entry
                .attributes
                .as_slice()
                .try_into()
                .ok()
                .map(u32::from_be_bytes)
            else {
                return Vec::new();
            };
            if segment_id != entry.segment_id
                || segment_type != attribute_type
                || header_byte_len != entry.segment_byte_len
            {
                return Vec::new();
            }
            let payload = &segment[24..];
            let compression = if payload.get(..4) == Some(2_u32.to_le_bytes().as_slice()) {
                let Some(compressed_data_byte_len) = payload
                    .get(4..8)
                    .and_then(|value| value.try_into().ok())
                    .map(u32::from_le_bytes)
                else {
                    return Vec::new();
                };
                let Some(&algorithm) = payload.get(8) else {
                    return Vec::new();
                };
                if algorithm != 2 {
                    return Vec::new();
                }
                let compressed = &payload[9..];
                if compressed_data_byte_len as usize != compressed.len() + 1 {
                    return Vec::new();
                }
                let mut decoder = ZlibDecoder::new(compressed);
                let mut inflated = Vec::new();
                if decoder.read_to_end(&mut inflated).is_err()
                    || decoder.total_in() != compressed.len() as u64
                {
                    return Vec::new();
                }
                let Ok(compressed_byte_len) = u32::try_from(compressed.len()) else {
                    return Vec::new();
                };
                Some(DisplayJtCompression {
                    flag: 2,
                    compressed_data_byte_len,
                    algorithm,
                    compressed_byte_len,
                    inflated_sha256: sha256_hex(&inflated),
                })
            } else {
                None
            };
            segments.push(DisplayJtSegment {
                id: format!("nx:display-jt:segment#{document_key}-{}", entry.ordinal),
                document: document.id.clone(),
                toc_entry: entry.id.clone(),
                segment_id: segment_id.to_vec(),
                segment_type,
                segment_byte_len: header_byte_len,
                payload_sha256: sha256_hex(payload),
                compression,
                source_offset: document.source_offset + u64::from(entry.segment_offset),
            });
        }
    }
    segments
}

/// Decode complete object-element sequences from type-7 shape-LOD segments.
pub fn display_jt_shape_lod_elements(
    container: &Container,
    segments: &[DisplayJtSegment],
) -> Vec<DisplayJtShapeLodElement> {
    const SEGMENT_TAIL: [u8; 6] = [1, 0, 0, 0, 0, 0];
    let mut elements = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 7) {
        let Ok(start) = usize::try_from(segment.source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(segment.segment_byte_len as usize))
        else {
            return Vec::new();
        };
        let payload = &bytes[24..];
        let Some((parsed, framed_end)) = parse_jt_element_sequence(payload) else {
            return Vec::new();
        };
        if payload.get(framed_end..) != Some(SEGMENT_TAIL.as_slice()) {
            return Vec::new();
        }
        for (ordinal, element) in parsed.into_iter().enumerate() {
            if element.object_base_type != 4 {
                return Vec::new();
            }
            elements.push(DisplayJtShapeLodElement {
                id: format!("{}-element-{ordinal}", segment.id),
                segment: segment.id.clone(),
                ordinal: ordinal as u32,
                object_type_id: element.object_type_id.to_vec(),
                object_id: element.object_id,
                object_base_type: element.object_base_type,
                body_byte_len: element.body.len() as u32,
                body_sha256: sha256_hex(element.body),
                source_offset: segment.source_offset + 24 + element.offset as u64,
            });
        }
    }
    elements
}

/// Decode fixed headers from JT 9 tri-strip shape-LOD elements.
pub fn display_jt_tri_strip_lod_headers(
    container: &Container,
    elements: &[DisplayJtShapeLodElement],
) -> Vec<DisplayJtTriStripLodHeader> {
    const TRI_STRIP_LOD_TYPE: [u8; 16] = [
        0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut headers = Vec::new();
    for element in elements
        .iter()
        .filter(|element| element.object_type_id == TRI_STRIP_LOD_TYPE)
    {
        let Ok(body_start) = usize::try_from(element.source_offset + 25) else {
            return Vec::new();
        };
        let Some(body) = container
            .data
            .get(body_start..body_start.saturating_add(element.body_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some((
            vertex_bindings,
            topological_mesh_version,
            vertex_records_object_id,
            compressed_lod_version,
            compressed_representation,
        )) = parse_jt9_tri_strip_lod_header(body)
        else {
            return Vec::new();
        };
        headers.push(DisplayJtTriStripLodHeader {
            id: format!("{}-tri-strip-header", element.id),
            element: element.id.clone(),
            base_version: 1,
            vertex_version: 1,
            vertex_bindings,
            topological_mesh_version,
            vertex_records_object_id,
            compressed_lod_version,
            compressed_representation_byte_len: compressed_representation.len() as u32,
            compressed_representation_sha256: sha256_hex(compressed_representation),
            source_offset: element.source_offset + 25,
        });
    }
    headers
}

/// Decode the initial face-degree packet from each JT 9 topological mesh.
pub fn display_jt_initial_face_degree_symbols(
    container: &Container,
    elements: &[DisplayJtShapeLodElement],
) -> Vec<DisplayJtInitialFaceDegreeSymbols> {
    const TRI_STRIP_LOD_TYPE: [u8; 16] = [
        0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut vectors = Vec::new();
    for element in elements
        .iter()
        .filter(|element| element.object_type_id == TRI_STRIP_LOD_TYPE)
    {
        let Ok(body_start) = usize::try_from(element.source_offset + 25) else {
            return Vec::new();
        };
        let Some(body) = container
            .data
            .get(body_start..body_start.saturating_add(element.body_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some((_, _, _, _, representation)) = parse_jt9_tri_strip_lod_header(body) else {
            return Vec::new();
        };
        let Some((residuals, packet_byte_len)) = crate::jt::decode_int32_cdp2(representation, 0)
        else {
            return Vec::new();
        };
        let degrees = crate::jt::unpack_predictor_residuals(&residuals, crate::jt::Predictor::Null);
        let Some(packet) = representation.get(..packet_byte_len) else {
            return Vec::new();
        };
        vectors.push(DisplayJtInitialFaceDegreeSymbols {
            id: format!("{}-initial-face-degrees", element.id),
            element: element.id.clone(),
            degrees,
            packet_byte_len: packet_byte_len as u32,
            packet_sha256: sha256_hex(packet),
            source_offset: element.source_offset + 45,
        });
    }
    vectors
}

/// Bound every JT 9 topology vector and decode the following vertex-record header.
pub fn display_jt_topology_packet_sequences(
    container: &Container,
    elements: &[DisplayJtShapeLodElement],
) -> (
    Vec<DisplayJtTopologyPacketSequence>,
    Vec<DisplayJtCompressedVertexRecordsHeader>,
    Vec<DisplayJtVertexCoordinateArrayHeader>,
) {
    const TRI_STRIP_LOD_TYPE: [u8; 16] = [
        0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const PREFIX_ROLES: [&str; 21] = [
        "face_degrees_0",
        "face_degrees_1",
        "face_degrees_2",
        "face_degrees_3",
        "face_degrees_4",
        "face_degrees_5",
        "face_degrees_6",
        "face_degrees_7",
        "vertex_valences",
        "vertex_groups",
        "vertex_flags",
        "face_attribute_masks_0",
        "face_attribute_masks_1",
        "face_attribute_masks_2",
        "face_attribute_masks_3",
        "face_attribute_masks_4",
        "face_attribute_masks_5",
        "face_attribute_masks_6",
        "face_attribute_masks_7",
        "face_attribute_masks_7_next_30",
        "face_attribute_masks_7_upper_4",
    ];
    const SPLIT_ROLES: [&str; 2] = ["split_face_symbols", "split_face_positions"];
    let mut sequences = Vec::new();
    let mut headers = Vec::new();
    let mut coordinate_headers = Vec::new();
    for element in elements
        .iter()
        .filter(|element| element.object_type_id == TRI_STRIP_LOD_TYPE)
    {
        let Ok(body_start) = usize::try_from(element.source_offset + 25) else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let Some(body) = container
            .data
            .get(body_start..body_start.saturating_add(element.body_byte_len as usize))
        else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let Some((lod_vertex_bindings, _, _, _, representation)) =
            parse_jt9_tri_strip_lod_header(body)
        else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let mut cursor = 0usize;
        let Some(high_degree_lane_count) =
            jt9_topology_high_degree_lane_count(representation, lod_vertex_bindings)
        else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let mut roles = Vec::with_capacity(23 + high_degree_lane_count);
        roles.extend(PREFIX_ROLES.map(str::to_string));
        roles.extend(
            (0..high_degree_lane_count)
                .map(|ordinal| format!("high_degree_face_attribute_masks_{ordinal}")),
        );
        roles.extend(SPLIT_ROLES.map(str::to_string));
        let mut packets = Vec::with_capacity(roles.len());
        for role in roles {
            let Some(remaining) = representation.get(cursor..) else {
                return (Vec::new(), Vec::new(), Vec::new());
            };
            let Some((value_count, codec, byte_len)) = crate::jt::frame_int32_cdp2(remaining, 0)
            else {
                return (Vec::new(), Vec::new(), Vec::new());
            };
            let Some(packet_end) = cursor.checked_add(byte_len) else {
                return (Vec::new(), Vec::new(), Vec::new());
            };
            let Some(packet) = representation.get(cursor..packet_end) else {
                return (Vec::new(), Vec::new(), Vec::new());
            };
            let (Ok(byte_len), Ok(representation_offset)) =
                (u32::try_from(byte_len), u32::try_from(cursor))
            else {
                return (Vec::new(), Vec::new(), Vec::new());
            };
            let values = crate::jt::decode_int32_cdp2(packet, 0).and_then(
                |(residuals, decoded_byte_len)| {
                    (decoded_byte_len == packet.len()).then(|| {
                        let predictor = match role.as_str() {
                            "vertex_flags" | "split_face_symbols" => crate::jt::Predictor::Lag1,
                            _ => crate::jt::Predictor::Null,
                        };
                        crate::jt::unpack_predictor_residuals(&residuals, predictor)
                    })
                },
            );
            packets.push(DisplayJtTopologyPacket {
                role,
                value_count,
                codec,
                byte_len,
                sha256: sha256_hex(packet),
                representation_offset,
                values,
            });
            cursor += byte_len as usize;
        }
        let Some(hash_end) = cursor.checked_add(4) else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let Some(composite_hash) = representation
            .get(cursor..hash_end)
            .and_then(|value| value.try_into().ok())
            .map(u32::from_le_bytes)
        else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        cursor += 4;
        let Some(required_header_end) = cursor.checked_add(16) else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let Some(required_header) = representation.get(cursor..required_header_end) else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let vertex_bindings = u64::from_le_bytes(required_header[..8].try_into().expect("fixed"));
        let quantization = &required_header[8..12];
        if quantization[0] > 24
            || quantization[1] > 13
            || quantization[2] > 24
            || quantization[3] > 24
        {
            return (Vec::new(), Vec::new(), Vec::new());
        }
        if vertex_bindings != lod_vertex_bindings {
            return (Vec::new(), Vec::new(), Vec::new());
        }
        let topological_vertex_count =
            u32::from_le_bytes(required_header[12..16].try_into().expect("fixed"));
        let (vertex_attribute_count, vertex_header_byte_len) = if topological_vertex_count == 0 {
            (0, 16)
        } else {
            let Some(attribute_end) = cursor.checked_add(20) else {
                return (Vec::new(), Vec::new(), Vec::new());
            };
            let Some(attribute_bytes) = representation.get(cursor + 16..attribute_end) else {
                return (Vec::new(), Vec::new(), Vec::new());
            };
            (
                u32::from_le_bytes(attribute_bytes.try_into().expect("fixed")),
                20,
            )
        };
        if i32::try_from(topological_vertex_count).is_err()
            || i32::try_from(vertex_attribute_count).is_err()
        {
            return (Vec::new(), Vec::new(), Vec::new());
        }
        let arrays = &representation[cursor + vertex_header_byte_len..];
        let (Ok(topology_byte_len), Ok(compressed_arrays_byte_len)) =
            (u32::try_from(cursor), u32::try_from(arrays.len()))
        else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let representation_source_offset = element.source_offset + 45;
        if topological_vertex_count != 0 {
            let Some(coordinate_header) = arrays.get(..32) else {
                return (Vec::new(), Vec::new(), Vec::new());
            };
            let unique_vertex_count =
                u32::from_le_bytes(coordinate_header[..4].try_into().expect("fixed"));
            let component_count = coordinate_header[4];
            if unique_vertex_count != topological_vertex_count || component_count != 3 {
                return (Vec::new(), Vec::new(), Vec::new());
            }
            let mut component_ranges = [[0.0; 2]; 3];
            let mut component_quantization_bits = [0; 3];
            for component in 0..3 {
                let offset = 5 + component * 9;
                let minimum = f32::from_le_bytes(
                    coordinate_header[offset..offset + 4]
                        .try_into()
                        .expect("fixed"),
                );
                let maximum = f32::from_le_bytes(
                    coordinate_header[offset + 4..offset + 8]
                        .try_into()
                        .expect("fixed"),
                );
                let bits = coordinate_header[offset + 8];
                if !minimum.is_finite()
                    || !maximum.is_finite()
                    || minimum > maximum
                    || bits > 32
                    || bits != quantization[0]
                {
                    return (Vec::new(), Vec::new(), Vec::new());
                }
                component_ranges[component] = [minimum, maximum];
                component_quantization_bits[component] = bits;
            }
            let compressed_components = &arrays[32..];
            let Ok(compressed_components_byte_len) = u32::try_from(compressed_components.len())
            else {
                return (Vec::new(), Vec::new(), Vec::new());
            };
            let Ok(vertex_header_byte_len_u64) = u64::try_from(vertex_header_byte_len) else {
                return (Vec::new(), Vec::new(), Vec::new());
            };
            coordinate_headers.push(DisplayJtVertexCoordinateArrayHeader {
                id: format!("{}-coordinate-array-header", element.id),
                element: element.id.clone(),
                unique_vertex_count,
                component_count,
                component_ranges,
                component_quantization_bits,
                compressed_components_byte_len,
                compressed_components_sha256: sha256_hex(compressed_components),
                source_offset: representation_source_offset
                    + u64::from(topology_byte_len)
                    + vertex_header_byte_len_u64,
            });
        }
        sequences.push(DisplayJtTopologyPacketSequence {
            id: format!("{}-topology-packets", element.id),
            element: element.id.clone(),
            packets,
            composite_hash,
            topology_byte_len,
            source_offset: representation_source_offset,
        });
        headers.push(DisplayJtCompressedVertexRecordsHeader {
            id: format!("{}-vertex-records-header", element.id),
            element: element.id.clone(),
            vertex_bindings,
            vertex_quantization_bits: quantization[0],
            normal_quantization_factor: quantization[1],
            texture_quantization_bits: quantization[2],
            color_quantization_bits: quantization[3],
            topological_vertex_count,
            vertex_attribute_count,
            compressed_arrays_byte_len,
            compressed_arrays_sha256: sha256_hex(arrays),
            source_offset: representation_source_offset + u64::from(topology_byte_len),
        });
    }
    (sequences, headers, coordinate_headers)
}

/// Decode every complete JT 9 coordinate array.
pub fn display_jt_vertex_coordinates(
    container: &Container,
    headers: &[DisplayJtVertexCoordinateArrayHeader],
) -> Vec<DisplayJtVertexCoordinates> {
    let mut arrays = Vec::new();
    for header in headers {
        let Ok(start) = usize::try_from(header.source_offset + 32) else {
            return Vec::new();
        };
        let Ok(byte_len) = usize::try_from(header.compressed_components_byte_len) else {
            return Vec::new();
        };
        let Some(bytes) = container.data.get(start..start.saturating_add(byte_len)) else {
            return Vec::new();
        };
        let Some((points_m, coordinate_hash, consumed)) = crate::jt::decode_vertex_coordinates(
            bytes,
            header.unique_vertex_count as usize,
            header.component_ranges,
            header.component_quantization_bits,
        ) else {
            return Vec::new();
        };
        let Ok(consumed) = u32::try_from(consumed) else {
            return Vec::new();
        };
        arrays.push(DisplayJtVertexCoordinates {
            id: header
                .id
                .replacen("coordinate-array-header", "vertex-coordinates", 1),
            header: header.id.clone(),
            points_m,
            coordinate_hash,
            byte_len: consumed,
            source_offset: header.source_offset + 32,
        });
    }
    arrays
}

/// Reconstruct every complete JT 9 polygon mesh from its dual-mesh lanes.
pub fn display_jt_polygon_meshes(
    sequences: &[DisplayJtTopologyPacketSequence],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
) -> Vec<DisplayJtPolygonMesh> {
    let mut meshes = Vec::new();
    for sequence in sequences {
        let values = |role: &str| {
            sequence
                .packets
                .iter()
                .find(|packet| packet.role == role)?
                .values
                .as_deref()
        };
        let Some(valences) = values("vertex_valences") else {
            return Vec::new();
        };
        if valences.is_empty() {
            continue;
        }
        let Some(coordinate_header) = coordinate_headers
            .iter()
            .find(|header| header.element == sequence.element)
        else {
            return Vec::new();
        };
        let Some(degrees) = (0..8)
            .map(|context| values(&format!("face_degrees_{context}")))
            .collect::<Option<Vec<_>>>()
        else {
            return Vec::new();
        };
        let Some(attribute_masks) = (0..8)
            .map(|context| values(&format!("face_attribute_masks_{context}")))
            .collect::<Option<Vec<_>>>()
        else {
            return Vec::new();
        };
        let Some(context_7_next_30) = values("face_attribute_masks_7_next_30") else {
            return Vec::new();
        };
        let Some(context_7_upper_4) = values("face_attribute_masks_7_upper_4") else {
            return Vec::new();
        };
        let Some(large_lanes) = sequence
            .packets
            .iter()
            .filter(|packet| packet.role.starts_with("high_degree_face_attribute_masks_"))
            .map(|packet| packet.values.as_deref())
            .collect::<Option<Vec<_>>>()
        else {
            return Vec::new();
        };
        let large_words = large_lanes
            .into_iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let Some(polygons) = crate::jt_topology::decode(
            degrees.try_into().expect("eight degree contexts"),
            valences,
            values("vertex_groups").unwrap_or_default(),
            values("vertex_flags").unwrap_or_default(),
            values("split_face_symbols").unwrap_or_default(),
            values("split_face_positions").unwrap_or_default(),
            crate::jt_topology::AttributeMaskLanes {
                small: attribute_masks
                    .try_into()
                    .expect("eight attribute-mask contexts"),
                context_7_next_30,
                context_7_upper_4,
                large_words: &large_words,
            },
        ) else {
            return Vec::new();
        };
        if polygons.iter().any(|polygon| {
            polygon
                .vertex_indices
                .iter()
                .any(|&index| index >= coordinate_header.unique_vertex_count)
        }) {
            return Vec::new();
        }
        meshes.push(DisplayJtPolygonMesh {
            id: sequence.id.replacen("topology-packets", "polygon-mesh", 1),
            topology: sequence.id.clone(),
            coordinate_header: coordinate_header.id.clone(),
            polygon_groups: polygons.iter().map(|polygon| polygon.group).collect(),
            polygon_flags: polygons.iter().map(|polygon| polygon.flags).collect(),
            vertex_attribute_indices: polygons
                .iter()
                .map(|polygon| polygon.attribute_indices.clone())
                .collect(),
            polygons: polygons
                .into_iter()
                .map(|polygon| polygon.vertex_indices)
                .collect(),
            source_offset: sequence.source_offset,
        });
    }
    meshes
}

/// Decode every complete JT 9 normal array following a coordinate array.
pub fn display_jt_vertex_normals(
    container: &Container,
    vertex_headers: &[DisplayJtCompressedVertexRecordsHeader],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
    coordinates: &[DisplayJtVertexCoordinates],
) -> Vec<DisplayJtVertexNormals> {
    let mut arrays = Vec::new();
    for vertex_header in vertex_headers {
        if vertex_header.vertex_attribute_count == 0 || vertex_header.vertex_bindings & 0x8 == 0 {
            continue;
        }
        let Some(coordinate_header) = coordinate_headers
            .iter()
            .find(|header| header.element == vertex_header.element)
        else {
            return Vec::new();
        };
        let Some(coordinates) = coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id)
        else {
            return Vec::new();
        };
        let Some(source_offset) = coordinates
            .source_offset
            .checked_add(u64::from(coordinates.byte_len))
        else {
            return Vec::new();
        };
        let Ok(start) = usize::try_from(source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container.data.get(start..) else {
            return Vec::new();
        };
        let Some((normals, normal_hash, byte_len)) = crate::jt::decode_vertex_normals(
            bytes,
            vertex_header.vertex_attribute_count as usize,
            vertex_header.normal_quantization_factor,
        ) else {
            return Vec::new();
        };
        let Ok(byte_len) = u32::try_from(byte_len) else {
            return Vec::new();
        };
        arrays.push(DisplayJtVertexNormals {
            id: format!("{}-vertex-normals", vertex_header.element),
            vertex_records_header: vertex_header.id.clone(),
            normals,
            normal_hash,
            byte_len,
            source_offset,
        });
    }
    arrays
}

/// Decode every complete JT 9 color array after coordinates and optional normals.
pub fn display_jt_vertex_colors(
    container: &Container,
    vertex_headers: &[DisplayJtCompressedVertexRecordsHeader],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
    coordinates: &[DisplayJtVertexCoordinates],
    normals: &[DisplayJtVertexNormals],
) -> Vec<DisplayJtVertexColors> {
    let mut arrays = Vec::new();
    for vertex_header in vertex_headers {
        if vertex_header.vertex_attribute_count == 0 || vertex_header.vertex_bindings & 0x30 == 0 {
            continue;
        }
        let Some(coordinate_header) = coordinate_headers
            .iter()
            .find(|header| header.element == vertex_header.element)
        else {
            return Vec::new();
        };
        let Some(coordinates) = coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id)
        else {
            return Vec::new();
        };
        let Some(mut source_offset) = coordinates
            .source_offset
            .checked_add(u64::from(coordinates.byte_len))
        else {
            return Vec::new();
        };
        if vertex_header.vertex_bindings & 0x8 != 0 {
            let Some(normal_array) = normals
                .iter()
                .find(|normal| normal.vertex_records_header == vertex_header.id)
            else {
                return Vec::new();
            };
            let Some(next) = source_offset.checked_add(u64::from(normal_array.byte_len)) else {
                return Vec::new();
            };
            source_offset = next;
        }
        let Ok(start) = usize::try_from(source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container.data.get(start..) else {
            return Vec::new();
        };
        let Some((colors, color_hash, byte_len)) = crate::jt::decode_vertex_colors(
            bytes,
            vertex_header.vertex_attribute_count as usize,
            vertex_header.color_quantization_bits,
        ) else {
            return Vec::new();
        };
        let Ok(byte_len) = u32::try_from(byte_len) else {
            return Vec::new();
        };
        arrays.push(DisplayJtVertexColors {
            id: format!("{}-vertex-colors", vertex_header.element),
            vertex_records_header: vertex_header.id.clone(),
            colors,
            color_hash,
            byte_len,
            source_offset,
        });
    }
    arrays
}

/// Decode texture-coordinate channels after preceding coordinate, normal, and color arrays.
pub fn display_jt_vertex_texture_coordinates(
    container: &Container,
    vertex_headers: &[DisplayJtCompressedVertexRecordsHeader],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
    coordinates: &[DisplayJtVertexCoordinates],
    normals: &[DisplayJtVertexNormals],
    colors: &[DisplayJtVertexColors],
) -> Vec<DisplayJtVertexTextureCoordinates> {
    let mut arrays = Vec::new();
    for vertex_header in vertex_headers {
        let texture_channels = (0..8)
            .filter(|channel| vertex_header.vertex_bindings & (0xf_u64 << (8 + 4 * channel)) != 0)
            .collect::<Vec<_>>();
        if texture_channels.is_empty() {
            continue;
        }
        if vertex_header.vertex_attribute_count == 0 {
            return Vec::new();
        }
        let Some(coordinate_header) = coordinate_headers
            .iter()
            .find(|header| header.element == vertex_header.element)
        else {
            return Vec::new();
        };
        let Some(coordinates) = coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id)
        else {
            return Vec::new();
        };
        let Some(mut source_offset) = coordinates
            .source_offset
            .checked_add(u64::from(coordinates.byte_len))
        else {
            return Vec::new();
        };
        if vertex_header.vertex_bindings & 0x8 != 0 {
            let Some(normal_array) = normals
                .iter()
                .find(|normal| normal.vertex_records_header == vertex_header.id)
            else {
                return Vec::new();
            };
            let Some(next) = source_offset.checked_add(u64::from(normal_array.byte_len)) else {
                return Vec::new();
            };
            source_offset = next;
        }
        if vertex_header.vertex_bindings & 0x30 != 0 {
            let Some(color_array) = colors
                .iter()
                .find(|color| color.vertex_records_header == vertex_header.id)
            else {
                return Vec::new();
            };
            let Some(next) = source_offset.checked_add(u64::from(color_array.byte_len)) else {
                return Vec::new();
            };
            source_offset = next;
        }
        for channel in texture_channels {
            let Ok(start) = usize::try_from(source_offset) else {
                return Vec::new();
            };
            let Some(bytes) = container.data.get(start..) else {
                return Vec::new();
            };
            let Some((values, texture_coordinate_hash, byte_len)) =
                crate::jt::decode_vertex_texture_coordinates(
                    bytes,
                    vertex_header.vertex_attribute_count as usize,
                    vertex_header.texture_quantization_bits,
                )
            else {
                return Vec::new();
            };
            let Ok(byte_len) = u32::try_from(byte_len) else {
                return Vec::new();
            };
            arrays.push(DisplayJtVertexTextureCoordinates {
                id: format!("{}-texture-coordinates-{channel}", vertex_header.element),
                vertex_records_header: vertex_header.id.clone(),
                channel: channel as u8,
                values,
                texture_coordinate_hash,
                byte_len,
                source_offset,
            });
            let Some(next) = source_offset.checked_add(u64::from(byte_len)) else {
                return Vec::new();
            };
            source_offset = next;
        }
    }
    arrays
}

/// Decode every complete JT 9 vertex-flag array after all preceding vertex arrays.
pub fn display_jt_vertex_flags(
    container: &Container,
    vertex_headers: &[DisplayJtCompressedVertexRecordsHeader],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
    coordinates: &[DisplayJtVertexCoordinates],
    normals: &[DisplayJtVertexNormals],
    colors: &[DisplayJtVertexColors],
    texture_coordinates: &[DisplayJtVertexTextureCoordinates],
) -> Vec<DisplayJtVertexFlags> {
    let mut arrays = Vec::new();
    for vertex_header in vertex_headers {
        if vertex_header.vertex_attribute_count == 0 || vertex_header.vertex_bindings & 0x40 == 0 {
            continue;
        }
        let Some(coordinate_header) = coordinate_headers
            .iter()
            .find(|header| header.element == vertex_header.element)
        else {
            return Vec::new();
        };
        let Some(coordinates) = coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id)
        else {
            return Vec::new();
        };
        let Some(mut source_offset) = coordinates
            .source_offset
            .checked_add(u64::from(coordinates.byte_len))
        else {
            return Vec::new();
        };
        if vertex_header.vertex_bindings & 0x8 != 0 {
            let Some(array) = normals
                .iter()
                .find(|array| array.vertex_records_header == vertex_header.id)
            else {
                return Vec::new();
            };
            let Some(next) = source_offset.checked_add(u64::from(array.byte_len)) else {
                return Vec::new();
            };
            source_offset = next;
        }
        if vertex_header.vertex_bindings & 0x30 != 0 {
            let Some(array) = colors
                .iter()
                .find(|array| array.vertex_records_header == vertex_header.id)
            else {
                return Vec::new();
            };
            let Some(next) = source_offset.checked_add(u64::from(array.byte_len)) else {
                return Vec::new();
            };
            source_offset = next;
        }
        for channel in (0..8)
            .filter(|channel| vertex_header.vertex_bindings & (0xf_u64 << (8 + 4 * channel)) != 0)
        {
            let Some(array) = texture_coordinates.iter().find(|array| {
                array.vertex_records_header == vertex_header.id
                    && usize::from(array.channel) == channel
            }) else {
                return Vec::new();
            };
            let Some(next) = source_offset.checked_add(u64::from(array.byte_len)) else {
                return Vec::new();
            };
            source_offset = next;
        }
        let Ok(start) = usize::try_from(source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container.data.get(start..) else {
            return Vec::new();
        };
        let Some((values, byte_len)) =
            crate::jt::decode_vertex_flags(bytes, vertex_header.vertex_attribute_count as usize)
        else {
            return Vec::new();
        };
        let Ok(byte_len) = u32::try_from(byte_len) else {
            return Vec::new();
        };
        arrays.push(DisplayJtVertexFlags {
            id: format!("{}-vertex-flags", vertex_header.element),
            vertex_records_header: vertex_header.id.clone(),
            values,
            byte_len,
            source_offset,
        });
    }
    arrays
}

/// Decode element framing and exact post-marker tails from compressed segments.
pub fn display_jt_compressed_element_sequences(
    container: &Container,
    segments: &[DisplayJtSegment],
) -> (
    Vec<DisplayJtCompressedElement>,
    Vec<DisplayJtCompressedElementSequence>,
) {
    let mut elements = Vec::new();
    let mut sequences = Vec::new();
    for segment in segments
        .iter()
        .filter(|segment| segment.compression.is_some())
    {
        let Ok(start) = usize::try_from(segment.source_offset) else {
            return (Vec::new(), Vec::new());
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(segment.segment_byte_len as usize))
        else {
            return (Vec::new(), Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return (Vec::new(), Vec::new());
        };
        let mut decoder = ZlibDecoder::new(compressed);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err()
            || decoder.total_in() != compressed.len() as u64
        {
            return (Vec::new(), Vec::new());
        }
        let Some((parsed, framed_end)) = parse_jt_element_sequence(&inflated) else {
            return (Vec::new(), Vec::new());
        };
        let mut element_ids = Vec::with_capacity(parsed.len());
        for (ordinal, element) in parsed.into_iter().enumerate() {
            let id = format!("{}-inflated-element-{ordinal}", segment.id);
            element_ids.push(id.clone());
            elements.push(DisplayJtCompressedElement {
                id,
                segment: segment.id.clone(),
                segment_type: segment.segment_type,
                ordinal: ordinal as u32,
                object_type_id: element.object_type_id.to_vec(),
                object_id: element.object_id,
                object_base_type: element.object_base_type,
                body_byte_len: element.body.len() as u32,
                body_sha256: sha256_hex(element.body),
                inflated_offset: element.offset as u32,
                source_offset: segment.source_offset + 24,
            });
        }
        let tail = &inflated[framed_end..];
        sequences.push(DisplayJtCompressedElementSequence {
            id: format!("{}-inflated-sequence", segment.id),
            segment: segment.id.clone(),
            segment_type: segment.segment_type,
            elements: element_ids,
            framed_byte_len: framed_end as u32,
            tail: tail.to_vec(),
            tail_sha256: sha256_hex(tail),
            source_offset: segment.source_offset + 24,
        });
    }
    (elements, sequences)
}

/// Decode all string property atoms from complete type-31 segment sequences.
pub fn display_jt_string_property_atoms(
    container: &Container,
    segments: &[DisplayJtSegment],
) -> Vec<DisplayJtStringPropertyAtom> {
    const STRING_PROPERTY_ATOM_TYPE: [u8; 16] = [
        0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut atoms = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 31) {
        if segment.compression.is_none() {
            return Vec::new();
        }
        let Ok(start) = usize::try_from(segment.source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(segment.segment_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some(compressed) = bytes.get(33..) else {
            return Vec::new();
        };
        let mut decoder = ZlibDecoder::new(compressed);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err()
            || decoder.total_in() != compressed.len() as u64
        {
            return Vec::new();
        }
        let Some((elements, _)) = parse_jt_element_sequence(&inflated) else {
            return Vec::new();
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != STRING_PROPERTY_ATOM_TYPE || element.object_base_type != 5
            {
                return Vec::new();
            }
            let Some((code_units, value)) = parse_jt_string_property_atom_body(element.body) else {
                return Vec::new();
            };
            atoms.push(DisplayJtStringPropertyAtom {
                id: format!("{}-string-property-atom-{ordinal}", segment.id),
                element: format!("{}-inflated-element-{ordinal}", segment.id),
                object_id: element.object_id,
                code_units,
                value,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    atoms
}

/// Resolve JT 9 logical shape nodes to their late-loaded type-7 LOD segments.
pub fn display_jt_shape_lod_bindings(
    container: &Container,
    segments: &[DisplayJtSegment],
) -> Vec<DisplayJtShapeLodBinding> {
    const STRING_PROPERTY_ATOM_TYPE: [u8; 16] = [
        0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const LATE_LOADED_PROPERTY_ATOM_TYPE: [u8; 16] = [
        0xe5, 0x5b, 0xb0, 0xe0, 0xbd, 0xfb, 0xd1, 0x11, 0xa3, 0xa7, 0x00, 0xaa, 0x00, 0xd1, 0x09,
        0x54,
    ];
    const SHAPE_IMPLEMENTATION_KEY: &str = "JT_LLPROP_SHAPEIMPL";
    let read_u16 = |bytes: &[u8], offset: usize| {
        bytes
            .get(offset..offset + 2)
            .and_then(|value| value.try_into().ok())
            .map(u16::from_le_bytes)
    };
    let read_u32 = |bytes: &[u8], offset: usize| {
        bytes
            .get(offset..offset + 4)
            .and_then(|value| value.try_into().ok())
            .map(u32::from_le_bytes)
    };
    let mut bindings = Vec::new();
    for scene_segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Ok(start) = usize::try_from(scene_segment.source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(scene_segment.segment_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some(compressed) = bytes.get(33..) else {
            return Vec::new();
        };
        let mut decoder = ZlibDecoder::new(compressed);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err()
            || decoder.total_in() != compressed.len() as u64
        {
            return Vec::new();
        }
        let Some((_, scene_end)) = parse_jt_element_sequence(&inflated) else {
            return Vec::new();
        };
        let tail = &inflated[scene_end..];
        let Some((property_atoms, property_table_offset)) = parse_jt_element_sequence(tail) else {
            return Vec::new();
        };
        let mut strings = BTreeMap::new();
        let mut late_loaded = BTreeMap::new();
        for atom in property_atoms {
            if atom.object_type_id == STRING_PROPERTY_ATOM_TYPE && atom.object_base_type == 5 {
                let Some((_, value)) = parse_jt_string_property_atom_body(atom.body) else {
                    return Vec::new();
                };
                strings.insert(atom.object_id, value);
            } else if atom.object_type_id == LATE_LOADED_PROPERTY_ATOM_TYPE
                && atom.object_base_type == 8
            {
                if atom.body.len() != 36 || read_u16(atom.body, 0) != Some(1) {
                    return Vec::new();
                }
                let Some(state_flags) = read_u32(atom.body, 2) else {
                    return Vec::new();
                };
                let Some(property_version) = read_u16(atom.body, 6) else {
                    return Vec::new();
                };
                let segment_id = atom.body[8..24].to_vec();
                let Some(segment_type) = read_u32(atom.body, 24) else {
                    return Vec::new();
                };
                let Some(payload_object_id) = read_u32(atom.body, 28) else {
                    return Vec::new();
                };
                let Some(reserved_value) = read_u32(atom.body, 32).filter(|value| *value != 0)
                else {
                    return Vec::new();
                };
                late_loaded.insert(
                    atom.object_id,
                    (
                        state_flags,
                        property_version,
                        segment_id,
                        segment_type,
                        payload_object_id,
                        reserved_value,
                    ),
                );
            }
        }
        let table = &tail[property_table_offset..];
        let Some(table_version) = read_u16(table, 0) else {
            return Vec::new();
        };
        let Some(table_count) = read_u32(table, 2) else {
            return Vec::new();
        };
        let mut cursor = 6usize;
        for table_ordinal in 0..table_count {
            let Some(shape_node_object_id) = read_u32(table, cursor) else {
                return Vec::new();
            };
            cursor += 4;
            let mut pair_ordinal = 0u32;
            loop {
                let Some(key_object_id) = read_u32(table, cursor) else {
                    return Vec::new();
                };
                cursor += 4;
                if key_object_id == 0 {
                    break;
                }
                let Some(value_object_id) = read_u32(table, cursor) else {
                    return Vec::new();
                };
                cursor += 4;
                if strings.get(&key_object_id).map(String::as_str) == Some(SHAPE_IMPLEMENTATION_KEY)
                {
                    let Some((
                        state_flags,
                        property_version,
                        segment_id,
                        segment_type,
                        payload_object_id,
                        reserved_value,
                    )) = late_loaded.get(&value_object_id)
                    else {
                        return Vec::new();
                    };
                    let mut targets = segments.iter().filter(|segment| {
                        segment.document == scene_segment.document
                            && segment.segment_id == *segment_id
                            && segment.segment_type == *segment_type
                    });
                    let Some(target) = targets.next() else {
                        return Vec::new();
                    };
                    if targets.next().is_some() || target.segment_type != 7 {
                        return Vec::new();
                    }
                    bindings.push(DisplayJtShapeLodBinding {
                        id: format!(
                            "{}-shape-lod-binding-{table_ordinal}-{pair_ordinal}",
                            scene_segment.id
                        ),
                        scene_segment: scene_segment.id.clone(),
                        table_version,
                        shape_node_object_id,
                        key_object_id,
                        key: SHAPE_IMPLEMENTATION_KEY.to_string(),
                        value_object_id,
                        state_flags: *state_flags,
                        property_version: *property_version,
                        shape_segment: target.id.clone(),
                        payload_object_id: *payload_object_id,
                        reserved_value: *reserved_value,
                        source_offset: scene_segment.source_offset + 24,
                    });
                }
                pair_ordinal += 1;
            }
        }
        if cursor != table.len() {
            return Vec::new();
        }
    }
    bindings
}

/// Decode the common node-data header from every type-1 segment element.
pub fn display_jt_base_node_data(
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Vec<DisplayJtBaseNodeData> {
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Vec::new();
        };
        if segment.compression.is_none() {
            return Vec::new();
        }
        let Ok(start) = usize::try_from(segment.source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(segment.segment_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some(compressed) = bytes.get(33..) else {
            return Vec::new();
        };
        let mut decoder = ZlibDecoder::new(compressed);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err()
            || decoder.total_in() != compressed.len() as u64
        {
            return Vec::new();
        }
        let Some((elements, _)) = parse_jt_element_sequence(&inflated) else {
            return Vec::new();
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_base_type > 2 {
                continue;
            }
            let Some((version, flags, attribute_object_ids, family_data)) =
                parse_jt_base_node_body(element.body, document.format_major)
            else {
                return Vec::new();
            };
            nodes.push(DisplayJtBaseNodeData {
                id: format!("{}-base-node-{ordinal}", segment.id),
                element: format!("{}-inflated-element-{ordinal}", segment.id),
                object_type_id: element.object_type_id.to_vec(),
                object_id: element.object_id,
                version,
                flags,
                attribute_object_ids,
                family_data_byte_len: family_data.len() as u32,
                family_data_sha256: sha256_hex(family_data),
                source_offset: segment.source_offset + 24,
            });
        }
    }
    nodes
}

/// Decode common group-node data from every JT 9 group-derived scene node.
pub fn display_jt_group_node_data(
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Vec<DisplayJtGroupNodeData> {
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Vec::new();
        };
        if document.format_major != 9 || segment.compression.is_none() {
            continue;
        }
        let Ok(start) = usize::try_from(segment.source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(segment.segment_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some(compressed) = bytes.get(33..) else {
            return Vec::new();
        };
        let mut decoder = ZlibDecoder::new(compressed);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err()
            || decoder.total_in() != compressed.len() as u64
        {
            return Vec::new();
        }
        let Some((elements, _)) = parse_jt_element_sequence(&inflated) else {
            return Vec::new();
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_base_type != 1 {
                continue;
            }
            let Some((version, child_object_ids, family_data)) =
                parse_jt9_group_node_body(element.body)
            else {
                return Vec::new();
            };
            if version != 1 {
                return Vec::new();
            }
            nodes.push(DisplayJtGroupNodeData {
                id: format!("{}-group-node-data-{ordinal}", segment.id),
                base_node: format!("{}-base-node-{ordinal}", segment.id),
                object_id: element.object_id,
                version,
                child_object_ids,
                family_data_byte_len: family_data.len() as u32,
                family_data_sha256: sha256_hex(family_data),
                source_offset: segment.source_offset + 24,
            });
        }
    }
    nodes
}

/// Decode complete JT 9 instance nodes from logical scene-graph segments.
pub fn display_jt_instance_nodes(
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Vec<DisplayJtInstanceNode> {
    const INSTANCE_NODE_TYPE: [u8; 16] = [
        0x2a, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Vec::new();
        };
        if document.format_major != 9 || segment.compression.is_none() {
            continue;
        }
        let Ok(start) = usize::try_from(segment.source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(segment.segment_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some(compressed) = bytes.get(33..) else {
            return Vec::new();
        };
        let mut decoder = ZlibDecoder::new(compressed);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err()
            || decoder.total_in() != compressed.len() as u64
        {
            return Vec::new();
        }
        let Some((elements, _)) = parse_jt_element_sequence(&inflated) else {
            return Vec::new();
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != INSTANCE_NODE_TYPE {
                continue;
            }
            if element.object_base_type != 0 {
                return Vec::new();
            }
            let Some((version, child_object_id)) = parse_jt9_instance_node_body(element.body)
            else {
                return Vec::new();
            };
            nodes.push(DisplayJtInstanceNode {
                id: format!("{}-instance-node-{ordinal}", segment.id),
                base_node: format!("{}-base-node-{ordinal}", segment.id),
                object_id: element.object_id,
                version,
                child_object_id,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    nodes
}

/// Decode JT 9 geometric-transform attributes from logical scene-graph segments.
pub fn display_jt_geometric_transform_attributes(
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Vec<DisplayJtGeometricTransformAttribute> {
    const GEOMETRIC_TRANSFORM_TYPE: [u8; 16] = [
        0x83, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut attributes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Vec::new();
        };
        if document.format_major != 9 || segment.compression.is_none() {
            continue;
        }
        let Ok(start) = usize::try_from(segment.source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(segment.segment_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some(compressed) = bytes.get(33..) else {
            return Vec::new();
        };
        let mut decoder = ZlibDecoder::new(compressed);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err()
            || decoder.total_in() != compressed.len() as u64
        {
            return Vec::new();
        }
        let Some((elements, _)) = parse_jt_element_sequence(&inflated) else {
            return Vec::new();
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != GEOMETRIC_TRANSFORM_TYPE {
                continue;
            }
            if element.object_base_type != 3 {
                return Vec::new();
            }
            let Some((state_flags, field_inhibit_flags, stored_values_mask, matrix)) =
                parse_jt9_geometric_transform_body(element.body)
            else {
                return Vec::new();
            };
            attributes.push(DisplayJtGeometricTransformAttribute {
                id: format!("{}-geometric-transform-{ordinal}", segment.id),
                element: format!("{}-inflated-element-{ordinal}", segment.id),
                object_id: element.object_id,
                state_flags,
                field_inhibit_flags,
                stored_values_mask,
                matrix,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    attributes
}

/// Decode complete JT 9 partition nodes from logical scene-graph segments.
pub fn display_jt_partition_nodes(
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Vec<DisplayJtPartitionNode> {
    const PARTITION_NODE_TYPE: [u8; 16] = [
        0x3e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Vec::new();
        };
        if document.format_major >= 10 {
            continue;
        }
        let Ok(start) = usize::try_from(segment.source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(segment.segment_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some(compressed) = bytes.get(33..) else {
            return Vec::new();
        };
        let mut decoder = ZlibDecoder::new(compressed);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err()
            || decoder.total_in() != compressed.len() as u64
        {
            return Vec::new();
        }
        let Some((elements, _)) = parse_jt_element_sequence(&inflated) else {
            return Vec::new();
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != PARTITION_NODE_TYPE {
                continue;
            }
            let Some(node) = parse_jt9_partition_node_body(element.body) else {
                return Vec::new();
            };
            nodes.push(DisplayJtPartitionNode {
                id: format!("{}-partition-node-{ordinal}", segment.id),
                base_node: format!("{}-base-node-{ordinal}", segment.id),
                object_id: element.object_id,
                group_version: node.group_version,
                child_object_ids: node.child_object_ids,
                partition_flags: node.partition_flags,
                file_name_code_units: node.file_name_code_units,
                file_name: node.file_name,
                transformed_bounds: node.transformed_bounds,
                area: node.area,
                vertex_count_range: node.vertex_count_range,
                node_count_range: node.node_count_range,
                polygon_count_range: node.polygon_count_range,
                untransformed_bounds: node.untransformed_bounds,
                reserved_bounds: node.reserved_bounds,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    nodes
}

/// Decode complete JT 9 range-LOD nodes from logical scene-graph segments.
pub fn display_jt_range_lod_nodes(
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Vec<DisplayJtRangeLodNode> {
    const RANGE_LOD_NODE_TYPE: [u8; 16] = [
        0x4c, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Vec::new();
        };
        if document.format_major >= 10 {
            continue;
        }
        let Ok(start) = usize::try_from(segment.source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(segment.segment_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some(compressed) = bytes.get(33..) else {
            return Vec::new();
        };
        let mut decoder = ZlibDecoder::new(compressed);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err()
            || decoder.total_in() != compressed.len() as u64
        {
            return Vec::new();
        }
        let Some((elements, _)) = parse_jt_element_sequence(&inflated) else {
            return Vec::new();
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != RANGE_LOD_NODE_TYPE {
                continue;
            }
            let Some(node) = parse_jt9_range_lod_node_body(element.body) else {
                return Vec::new();
            };
            nodes.push(DisplayJtRangeLodNode {
                id: format!("{}-range-lod-node-{ordinal}", segment.id),
                base_node: format!("{}-base-node-{ordinal}", segment.id),
                object_id: element.object_id,
                group_version: node.group_version,
                child_object_ids: node.child_object_ids,
                lod_version: node.lod_version,
                reserved_values: node.reserved_values,
                reserved_value: node.reserved_value,
                range_version: node.range_version,
                range_limits: node.range_limits,
                center: node.center,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    nodes
}

/// Decode complete JT 9 tri-strip shape nodes from logical scene-graph segments.
pub fn display_jt_tri_strip_shape_nodes(
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Vec<DisplayJtTriStripShapeNode> {
    const TRI_STRIP_SHAPE_NODE_TYPE: [u8; 16] = [
        0x77, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Vec::new();
        };
        if document.format_major != 9 || segment.compression.is_none() {
            continue;
        }
        let Ok(start) = usize::try_from(segment.source_offset) else {
            return Vec::new();
        };
        let Some(bytes) = container
            .data
            .get(start..start.saturating_add(segment.segment_byte_len as usize))
        else {
            return Vec::new();
        };
        let Some(compressed) = bytes.get(33..) else {
            return Vec::new();
        };
        let mut decoder = ZlibDecoder::new(compressed);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err()
            || decoder.total_in() != compressed.len() as u64
        {
            return Vec::new();
        }
        let Some((elements, _)) = parse_jt_element_sequence(&inflated) else {
            return Vec::new();
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != TRI_STRIP_SHAPE_NODE_TYPE {
                continue;
            }
            if element.object_base_type != 2 {
                return Vec::new();
            }
            let Some(node) = parse_jt9_tri_strip_shape_node_body(element.body) else {
                return Vec::new();
            };
            nodes.push(DisplayJtTriStripShapeNode {
                id: format!("{}-tri-strip-shape-node-{ordinal}", segment.id),
                base_node: format!("{}-base-node-{ordinal}", segment.id),
                object_id: element.object_id,
                reserved_bounds: node.reserved_bounds,
                untransformed_bounds: node.untransformed_bounds,
                area: node.area,
                vertex_count_range: node.vertex_count_range,
                node_count_range: node.node_count_range,
                polygon_count_range: node.polygon_count_range,
                memory_byte_len: node.memory_byte_len,
                compression_level: node.compression_level,
                vertex_version: node.vertex_version,
                vertex_bindings: node.vertex_bindings,
                vertex_quantization_bits: node.vertex_quantization_bits,
                normal_quantization_factor: node.normal_quantization_factor,
                texture_quantization_bits: node.texture_quantization_bits,
                color_quantization_bits: node.color_quantization_bits,
                version_2_vertex_bindings: node.version_2_vertex_bindings,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    nodes
}

// --- JT display-model tessellation assembly (moved from decode.rs) ---

const DISPLAY_JT_COLOR_CHANNEL: u32 = 0x4e58_0001;
const DISPLAY_JT_VERTEX_FLAG_CHANNEL: u32 = 0x4e58_0002;
const DISPLAY_JT_TEXTURE_CHANNEL_BASE: u32 = 0x4e58_0100;
type DisplayJtMatrix = [[f64; 4]; 4];

struct DisplayJtPath {
    matrix: DisplayJtMatrix,
    final_transform: bool,
    node_path: Vec<u32>,
    instance_path: Vec<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct DisplayJtTessellationInputs<'a> {
    pub(crate) meshes: &'a [DisplayJtPolygonMesh],
    pub(crate) coordinates: &'a [DisplayJtVertexCoordinates],
    pub(crate) normals: &'a [DisplayJtVertexNormals],
    pub(crate) colors: &'a [DisplayJtVertexColors],
    pub(crate) texture_coordinates: &'a [DisplayJtVertexTextureCoordinates],
    pub(crate) vertex_flags: &'a [DisplayJtVertexFlags],
    pub(crate) vertex_headers: &'a [DisplayJtCompressedVertexRecordsHeader],
    pub(crate) coordinate_headers: &'a [DisplayJtVertexCoordinateArrayHeader],
    pub(crate) shape_elements: &'a [DisplayJtShapeLodElement],
    pub(crate) bindings: &'a [DisplayJtShapeLodBinding],
    pub(crate) shape_nodes: &'a [DisplayJtTriStripShapeNode],
    pub(crate) base_nodes: &'a [DisplayJtBaseNodeData],
    pub(crate) group_nodes: &'a [DisplayJtGroupNodeData],
    pub(crate) instance_nodes: &'a [DisplayJtInstanceNode],
    pub(crate) transforms: &'a [DisplayJtGeometricTransformAttribute],
    pub(crate) compressed_elements: &'a [DisplayJtCompressedElement],
}

fn multiply_jt_matrices(left: DisplayJtMatrix, right: DisplayJtMatrix) -> Option<DisplayJtMatrix> {
    let mut product = [[0.0; 4]; 4];
    for (row, values) in product.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = (0..4)
                .map(|inner| left[row][inner] * right[inner][column])
                .sum();
            if !value.is_finite() {
                return None;
            }
        }
    }
    Some(product)
}

fn resolve_display_jt_node_transform(
    object_id: u32,
    by_object: &BTreeMap<u32, &DisplayJtBaseNodeData>,
    parents: &BTreeMap<u32, Vec<u32>>,
    instance_ids: &BTreeMap<u32, String>,
    transforms: &[&DisplayJtGeometricTransformAttribute],
    visiting: &mut BTreeSet<u32>,
) -> Option<Vec<DisplayJtPath>> {
    let base = by_object.get(&object_id)?;
    if base.flags & 1 != 0 {
        return Some(Vec::new());
    }
    if !visiting.insert(object_id) {
        return None;
    }
    let parent_states = parents.get(&object_id).map_or_else(
        || {
            Some(vec![DisplayJtPath {
                matrix: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                final_transform: false,
                node_path: Vec::new(),
                instance_path: Vec::new(),
            }])
        },
        |ids| {
            ids.iter()
                .map(|id| {
                    resolve_display_jt_node_transform(
                        *id,
                        by_object,
                        parents,
                        instance_ids,
                        transforms,
                        visiting,
                    )
                })
                .collect::<Option<Vec<_>>>()
                .map(|paths| paths.into_iter().flatten().collect())
        },
    )?;
    visiting.remove(&object_id);
    let mut results = Vec::new();
    for mut path in parent_states {
        for attribute_id in &base.attribute_object_ids {
            let mut matching = transforms
                .iter()
                .filter(|attribute| attribute.object_id == *attribute_id);
            let attribute = matching.next()?;
            if matching.next().is_some() {
                return None;
            }
            if attribute.state_flags & 0x04 != 0 {
                continue;
            }
            if path.final_transform && attribute.state_flags & 0x02 == 0 {
                continue;
            }
            let local = attribute.matrix.map(|row| row.map(f64::from));
            path.matrix = multiply_jt_matrices(local, path.matrix)?;
            path.final_transform |= attribute.state_flags & 0x01 != 0;
        }
        if let Some(instance_id) = instance_ids.get(&object_id) {
            path.instance_path.push(instance_id.clone());
        }
        path.node_path.push(object_id);
        results.push(path);
    }
    Some(results)
}

fn display_jt_node_transform(
    scene_segment: &str,
    shape_object_id: u32,
    inputs: &DisplayJtTessellationInputs<'_>,
) -> Option<Vec<DisplayJtPath>> {
    let scoped = inputs
        .base_nodes
        .iter()
        .filter(|base| {
            inputs
                .compressed_elements
                .iter()
                .find(|element| element.id == base.element)
                .is_some_and(|element| element.segment == scene_segment)
        })
        .collect::<Vec<_>>();
    let mut by_object = BTreeMap::new();
    for base in &scoped {
        if by_object.insert(base.object_id, *base).is_some() {
            return None;
        }
    }
    by_object.get(&shape_object_id)?;
    let scoped_transforms = inputs
        .transforms
        .iter()
        .filter(|attribute| {
            inputs
                .compressed_elements
                .iter()
                .find(|element| element.id == attribute.element)
                .is_some_and(|element| element.segment == scene_segment)
        })
        .collect::<Vec<_>>();
    let mut parents = BTreeMap::<u32, Vec<u32>>::new();
    let mut instance_ids = BTreeMap::new();
    for node in inputs.instance_nodes {
        if !by_object.values().any(|base| base.id == node.base_node) {
            continue;
        }
        if instance_ids
            .insert(node.object_id, node.id.clone())
            .is_some()
        {
            return None;
        }
    }
    for (object_id, base) in &by_object {
        let group_children = inputs
            .group_nodes
            .iter()
            .filter(|node| node.base_node == base.id)
            .map(|node| node.child_object_ids.as_slice())
            .collect::<Vec<_>>();
        let instance_children = inputs
            .instance_nodes
            .iter()
            .filter(|node| node.base_node == base.id)
            .map(|node| std::slice::from_ref(&node.child_object_id))
            .collect::<Vec<_>>();
        if group_children.len() + instance_children.len() > 1 {
            return None;
        }
        let children = group_children
            .into_iter()
            .chain(instance_children)
            .next()
            .unwrap_or_default();
        for &child in children {
            by_object.get(&child)?;
            parents.entry(child).or_default().push(*object_id);
        }
    }
    resolve_display_jt_node_transform(
        shape_object_id,
        &by_object,
        &parents,
        &instance_ids,
        &scoped_transforms,
        &mut BTreeSet::new(),
    )
}

fn transform_jt_point(matrix: [[f64; 4]; 4], point: [f32; 3]) -> Option<Point3> {
    let point = point.map(f64::from);
    let coordinate = |column| {
        (matrix[3][column]
            + (0..3)
                .map(|row| point[row] * matrix[row][column])
                .sum::<f64>())
            * 1000.0
    };
    let point = Point3::new(coordinate(0), coordinate(1), coordinate(2));
    [point.x, point.y, point.z]
        .iter()
        .all(|value| value.is_finite())
        .then_some(point)
}

fn transform_jt_normal(matrix: [[f64; 4]; 4], normal: [f32; 3]) -> Option<Vector3> {
    let a = matrix;
    let determinant = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if !determinant.is_finite() || determinant == 0.0 {
        return None;
    }
    let inverse = [
        [
            (a[1][1] * a[2][2] - a[1][2] * a[2][1]) / determinant,
            (a[0][2] * a[2][1] - a[0][1] * a[2][2]) / determinant,
            (a[0][1] * a[1][2] - a[0][2] * a[1][1]) / determinant,
        ],
        [
            (a[1][2] * a[2][0] - a[1][0] * a[2][2]) / determinant,
            (a[0][0] * a[2][2] - a[0][2] * a[2][0]) / determinant,
            (a[0][2] * a[1][0] - a[0][0] * a[1][2]) / determinant,
        ],
        [
            (a[1][0] * a[2][1] - a[1][1] * a[2][0]) / determinant,
            (a[0][1] * a[2][0] - a[0][0] * a[2][1]) / determinant,
            (a[0][0] * a[1][1] - a[0][1] * a[1][0]) / determinant,
        ],
    ];
    let normal = normal.map(f64::from);
    let transformed = Vector3::new(
        (0..3).map(|index| normal[index] * inverse[0][index]).sum(),
        (0..3).map(|index| normal[index] * inverse[1][index]).sum(),
        (0..3).map(|index| normal[index] * inverse[2][index]).sum(),
    );
    let length = transformed.norm();
    (length.is_finite() && length > 0.0).then(|| {
        Vector3::new(
            transformed.x / length,
            transformed.y / length,
            transformed.z / length,
        )
    })
}

pub(crate) fn display_jt_tessellations(
    inputs: &DisplayJtTessellationInputs<'_>,
) -> Option<Vec<(Tessellation, u64)>> {
    let DisplayJtTessellationInputs {
        meshes,
        coordinates,
        normals,
        colors,
        texture_coordinates,
        vertex_flags,
        vertex_headers,
        coordinate_headers,
        shape_elements,
        bindings,
        shape_nodes,
        base_nodes,
        compressed_elements,
        ..
    } = *inputs;
    let mut tessellations = Vec::new();
    for mesh in meshes {
        let coordinate_header = coordinate_headers
            .iter()
            .find(|header| header.id == mesh.coordinate_header)?;
        let coordinates = coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id)?;
        let shape_element = shape_elements
            .iter()
            .find(|element| element.id == coordinate_header.element)?;
        let mut matching_bindings = bindings.iter().filter(|binding| {
            binding.shape_segment == shape_element.segment
                && binding.payload_object_id == shape_element.object_id
        });
        let binding = matching_bindings.next()?;
        if matching_bindings.next().is_some() {
            return None;
        }
        let mut matching_nodes = shape_nodes.iter().filter(|node| {
            if node.object_id != binding.shape_node_object_id {
                return false;
            }
            let Some(base) = base_nodes.iter().find(|base| base.id == node.base_node) else {
                return false;
            };
            compressed_elements
                .iter()
                .find(|element| element.id == base.element)
                .is_some_and(|element| element.segment == binding.scene_segment)
        });
        let shape_node = matching_nodes.next()?;
        if matching_nodes.next().is_some() {
            return None;
        }
        let paths =
            display_jt_node_transform(&binding.scene_segment, shape_node.object_id, inputs)?;
        let mut rendered = Vec::new();
        for ((polygon, attributes), &group) in mesh
            .polygons
            .iter()
            .zip(&mesh.vertex_attribute_indices)
            .zip(&mesh.polygon_groups)
        {
            if group < 0 {
                continue;
            }
            let triangle: [u32; 3] = polygon.as_slice().try_into().ok()?;
            let attributes: [Option<u32>; 3] = attributes.as_slice().try_into().ok()?;
            rendered.push((triangle, attributes));
        }
        if rendered.is_empty() {
            return None;
        }
        let vertex_header = vertex_headers
            .iter()
            .find(|header| header.element == shape_element.id)?;
        let normal_array = (vertex_header.vertex_bindings & 0x8 != 0)
            .then(|| {
                normals
                    .iter()
                    .find(|normals| normals.vertex_records_header == vertex_header.id)
            })
            .flatten();
        if vertex_header.vertex_bindings & 0x8 != 0 && normal_array.is_none() {
            return None;
        }
        let color_array = (vertex_header.vertex_bindings & 0x30 != 0)
            .then(|| {
                colors
                    .iter()
                    .find(|colors| colors.vertex_records_header == vertex_header.id)
            })
            .flatten();
        if vertex_header.vertex_bindings & 0x30 != 0 && color_array.is_none() {
            return None;
        }
        let texture_arrays = (0..8_u8)
            .filter(|channel| vertex_header.vertex_bindings & (0xf_u64 << (8 + 4 * channel)) != 0)
            .map(|channel| {
                texture_coordinates.iter().find(|coordinates| {
                    coordinates.vertex_records_header == vertex_header.id
                        && coordinates.channel == channel
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let vertex_flag_array = (vertex_header.vertex_bindings & 0x40 != 0)
            .then(|| {
                vertex_flags
                    .iter()
                    .find(|flags| flags.vertex_records_header == vertex_header.id)
            })
            .flatten();
        if vertex_header.vertex_bindings & 0x40 != 0 && vertex_flag_array.is_none() {
            return None;
        }
        for path in paths {
            let transform = path.matrix;
            let instance_path = path.instance_path;
            let node_path = path
                .node_path
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join("-");
            let convert_point = |index: u32| {
                let point = coordinates.points_m.get(index as usize)?;
                transform_jt_point(transform, *point)
            };
            let has_vertex_attributes = normal_array.is_some()
                || color_array.is_some()
                || !texture_arrays.is_empty()
                || vertex_flag_array.is_some();
            let (vertices, triangles, normal_vectors, channels) = if has_vertex_attributes {
                let mut vertices = Vec::with_capacity(rendered.len() * 3);
                let mut triangles = Vec::with_capacity(rendered.len());
                let mut normal_vectors = normal_array
                    .map(|_| Vec::with_capacity(rendered.len() * 3))
                    .unwrap_or_default();
                let mut color_data = color_array
                    .map(|_| Vec::with_capacity(rendered.len() * 3 * 16))
                    .unwrap_or_default();
                let texture_component_counts = texture_arrays
                    .iter()
                    .map(|array| {
                        let count = array.values.first()?.len();
                        (1..=4)
                            .contains(&count)
                            .then_some(count)
                            .filter(|count| array.values.iter().all(|value| value.len() == *count))
                    })
                    .collect::<Option<Vec<_>>>()?;
                let mut texture_data = texture_component_counts
                    .iter()
                    .map(|count| Vec::with_capacity(rendered.len() * 3 * count * 4))
                    .collect::<Vec<_>>();
                let mut vertex_flag_data = vertex_flag_array
                    .map(|_| Vec::with_capacity(rendered.len() * 3 * 4))
                    .unwrap_or_default();
                for (triangle, attributes) in rendered.iter().copied() {
                    let base = u32::try_from(vertices.len()).ok()?;
                    for (coordinate, attribute) in triangle.into_iter().zip(attributes) {
                        vertices.push(convert_point(coordinate)?);
                        let attribute = attribute? as usize;
                        if let Some(normal_array) = normal_array {
                            let normal = normal_array.normals.get(attribute)?;
                            normal_vectors.push(transform_jt_normal(transform, *normal)?);
                        }
                        if let Some(color_array) = color_array {
                            for component in color_array.colors.get(attribute)? {
                                color_data.extend_from_slice(&component.to_le_bytes());
                            }
                        }
                        for (array, data) in texture_arrays.iter().zip(&mut texture_data) {
                            for component in array.values.get(attribute)? {
                                data.extend_from_slice(&component.to_le_bytes());
                            }
                        }
                        if let Some(array) = vertex_flag_array {
                            vertex_flag_data
                                .extend_from_slice(&array.values.get(attribute)?.to_le_bytes());
                        }
                    }
                    triangles.push([base, base.checked_add(1)?, base.checked_add(2)?]);
                }
                let count = u32::try_from(vertices.len()).ok()?;
                let mut channels = Vec::new();
                if color_array.is_some() {
                    channels.push(TessellationChannel {
                        item_size: 16,
                        kind: DISPLAY_JT_COLOR_CHANNEL,
                        flags: ((vertex_header.vertex_bindings >> 4) & 0x3) as u32,
                        count,
                        data: color_data,
                    });
                }
                for (((array, component_count), data), ordinal) in texture_arrays
                    .iter()
                    .zip(texture_component_counts)
                    .zip(texture_data)
                    .zip(0_u32..)
                {
                    channels.push(TessellationChannel {
                        item_size: u32::try_from(component_count.checked_mul(4)?).ok()?,
                        kind: DISPLAY_JT_TEXTURE_CHANNEL_BASE.checked_add(ordinal)?,
                        flags: u32::from(array.channel)
                            | (((vertex_header.vertex_bindings >> (8 + 4 * array.channel)) & 0xf)
                                as u32)
                                << 8,
                        count,
                        data,
                    });
                }
                if vertex_flag_array.is_some() {
                    channels.push(TessellationChannel {
                        item_size: 4,
                        kind: DISPLAY_JT_VERTEX_FLAG_CHANNEL,
                        flags: 0,
                        count,
                        data: vertex_flag_data,
                    });
                }
                (vertices, triangles, normal_vectors, channels)
            } else {
                let vertices = (0..coordinates.points_m.len())
                    .map(|index| convert_point(index as u32))
                    .collect::<Option<Vec<_>>>()?;
                let triangles = rendered.iter().map(|(triangle, _)| *triangle).collect();
                (vertices, triangles, Vec::new(), Vec::new())
            };
            tessellations.push((
                Tessellation {
                    id: if path.node_path.len() == 1 {
                        format!(
                            "nx:display-jt:tessellation#{}-{}",
                            shape_element.source_offset, shape_element.object_id
                        )
                    } else {
                        format!(
                            "nx:display-jt:tessellation#{}-{}-path-{node_path}",
                            shape_element.source_offset, shape_element.object_id
                        )
                    },
                    body: None,
                    faces: Vec::new(),
                    chordal_deflection: None,
                    source_object: Some(SourceObjectAssociation {
                        format: "nx".to_string(),
                        object_id: shape_node.id.clone(),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path,
                    }),
                    vertices,
                    triangles,
                    strip_lengths: Vec::new(),
                    normals: normal_vectors,
                    channels,
                },
                shape_node.source_offset,
            ));
        }
    }
    Some(tessellations)
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use std::io::{Cursor, Write};

    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry,
        ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::report::LossCategory;
    use cadmpeg_ir::Exactness;

    use crate::container;
    use crate::parasolid::{self, StreamKind};
    use crate::test_support::*;
    use crate::NxCodec;

    use super::*;

    #[test]
    fn display_jt_index_requires_every_declared_header() {
        use crate::container::{Container, DirEntry, Region};

        let mut inflated = Vec::new();
        inflated.extend_from_slice(&24_u32.to_le_bytes());
        inflated.extend_from_slice(&[3; 16]);
        inflated.push(1);
        inflated.extend_from_slice(&5_u32.to_le_bytes());
        inflated.extend_from_slice(&[9, 8, 7]);
        inflated.extend_from_slice(&16_u32.to_le_bytes());
        inflated.extend_from_slice(&[0xff; 16]);
        inflated.extend_from_slice(&[6, 5]);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&inflated).unwrap();
        let compressed = encoder.finish().unwrap();
        let segment_byte_len = 24 + 9 + compressed.len() as u32;
        let mut data = Vec::new();
        data.extend_from_slice(&9_u32.to_le_bytes());
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&100_u32.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&28_u32.to_le_bytes());
        data.extend_from_slice(&[0; 4]);
        let mut version = [b' '; 80];
        version[..14].copy_from_slice(b"Version 9.4 JT");
        data.extend_from_slice(&version);
        data.push(0);
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&105_u32.to_le_bytes());
        data.extend_from_slice(&[1; 16]);
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&[2; 16]);
        data.extend_from_slice(&137_u32.to_le_bytes());
        data.extend_from_slice(&segment_byte_len.to_le_bytes());
        data.extend_from_slice(&1_u32.to_be_bytes());
        data.extend_from_slice(&[2; 16]);
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&segment_byte_len.to_le_bytes());
        data.extend_from_slice(&2_u32.to_le_bytes());
        data.extend_from_slice(&(compressed.len() as u32 + 1).to_le_bytes());
        data.push(2);
        data.extend_from_slice(&compressed);
        let container = Container {
            data: data.clone(),
            version: 6,
            file_tag: 0,
            footer_offset: 0,
            entries: vec![DirEntry {
                name: "/Root/UG_PART/DisplayJT".to_string(),
                region: Region::Footer,
                file_span: Some((0, data.len() as u64)),
            }],
        };
        let indices = super::display_jt_indices(&container);
        assert_eq!(indices[0].version, 9);
        assert_eq!(indices[0].declared_count, 1);
        assert_eq!(indices[0].rows[0].header_offset, 28);
        assert_eq!(indices[0].rows[0].value, 100);
        let documents = super::display_jt_documents(&container, &indices);
        assert_eq!(
            (documents[0].format_major, documents[0].format_minor),
            (9, 4)
        );
        assert_eq!(documents[0].toc_offset, 105);
        assert_eq!(
            documents[0].physical_byte_len,
            137 + u64::from(segment_byte_len)
        );
        assert_eq!(documents[0].toc_entries.len(), 1);
        assert_eq!(documents[0].toc_entries[0].segment_offset, 137);
        assert_eq!(
            documents[0].toc_entries[0].segment_byte_len,
            segment_byte_len
        );
        assert_eq!(documents[0].toc_entries[0].attributes, [0, 0, 0, 1]);
        let segments = super::display_jt_segments(&container, &documents);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].id.matches('#').count(), 1);
        assert!(!segments[0].id.contains(&documents[0].id));
        assert_eq!(segments[0].segment_type, 1);
        assert_eq!(segments[0].segment_byte_len, segment_byte_len);
        let compression = segments[0].compression.as_ref().unwrap();
        assert_eq!(
            compression.compressed_data_byte_len,
            compressed.len() as u32 + 1
        );
        assert_eq!(compression.compressed_byte_len, compressed.len() as u32);
        assert_eq!(
            compression.inflated_sha256,
            cadmpeg_ir::hash::sha256_hex(&inflated)
        );
        let (compressed_elements, sequences) =
            super::display_jt_compressed_element_sequences(&container, &segments);
        assert_eq!(compressed_elements.len(), 1);
        assert_eq!(compressed_elements[0].segment_type, 1);
        assert_eq!(compressed_elements[0].object_type_id, [3; 16]);
        assert_eq!(compressed_elements[0].object_id, 5);
        assert_eq!(compressed_elements[0].object_base_type, 1);
        assert_eq!(compressed_elements[0].body_byte_len, 3);
        assert_eq!(sequences.len(), 1);
        assert_eq!(sequences[0].framed_byte_len, 48);
        assert_eq!(sequences[0].tail, [6, 5]);

        let mut malformed_compression = container.clone();
        malformed_compression.data[193..197]
            .copy_from_slice(&(compressed.len() as u32 + 2).to_le_bytes());
        assert!(super::display_jt_segments(&malformed_compression, &documents).is_empty());

        let mut malformed = container;
        malformed.data[28] = b'X';
        assert!(super::display_jt_indices(&malformed).is_empty());
    }

    #[test]
    fn display_jt_shape_lod_requires_canonical_end_marker_and_tail() {
        use super::DisplayJtSegment;
        use crate::container::Container;

        let object_type_id = [0x5a; 16];
        let body = [9, 8, 7];
        let mut data = Vec::new();
        data.extend_from_slice(&[1; 16]);
        data.extend_from_slice(&7_u32.to_le_bytes());
        data.extend_from_slice(&78_u32.to_le_bytes());
        data.extend_from_slice(&24_u32.to_le_bytes());
        data.extend_from_slice(&object_type_id);
        data.push(4);
        data.extend_from_slice(&42_u32.to_le_bytes());
        data.extend_from_slice(&body);
        data.extend_from_slice(&16_u32.to_le_bytes());
        data.extend_from_slice(&[0xff; 16]);
        data.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
        let container = Container {
            data,
            version: 6,
            file_tag: 0,
            footer_offset: 0,
            entries: Vec::new(),
        };
        let segment = DisplayJtSegment {
            id: "segment".to_string(),
            document: "document".to_string(),
            toc_entry: "entry".to_string(),
            segment_id: vec![1; 16],
            segment_type: 7,
            segment_byte_len: 78,
            payload_sha256: String::new(),
            compression: None,
            source_offset: 0,
        };
        let elements =
            super::display_jt_shape_lod_elements(&container, std::slice::from_ref(&segment));
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].object_type_id, object_type_id);
        assert_eq!(elements[0].object_id, 42);
        assert_eq!(elements[0].object_base_type, 4);
        assert_eq!(elements[0].body_byte_len, 3);

        let mut malformed = container;
        *malformed.data.last_mut().unwrap() = 1;
        assert!(super::display_jt_shape_lod_elements(&malformed, &[segment]).is_empty());
    }

    #[test]
    fn display_jt_shape_lod_binding_resolves_property_table_segment_reference() {
        use super::DisplayJtSegment;
        use crate::container::Container;

        let mut inflated = Vec::new();
        inflated.extend_from_slice(&16_u32.to_le_bytes());
        inflated.extend_from_slice(&[0xff; 16]);

        let mut late_body = vec![1, 0];
        late_body.extend_from_slice(&0x4000_0000_u32.to_le_bytes());
        late_body.extend_from_slice(&1_u16.to_le_bytes());
        late_body.extend_from_slice(&[9; 16]);
        late_body.extend_from_slice(&7_u32.to_le_bytes());
        late_body.extend_from_slice(&12_u32.to_le_bytes());
        late_body.extend_from_slice(&1_u32.to_le_bytes());
        inflated.extend_from_slice(&57_u32.to_le_bytes());
        inflated.extend_from_slice(&[
            0xe5, 0x5b, 0xb0, 0xe0, 0xbd, 0xfb, 0xd1, 0x11, 0xa3, 0xa7, 0x00, 0xaa, 0x00, 0xd1,
            0x09, 0x54,
        ]);
        inflated.push(8);
        inflated.extend_from_slice(&3_u32.to_le_bytes());
        inflated.extend_from_slice(&late_body);

        let key = "JT_LLPROP_SHAPEIMPL";
        let mut string_body = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
        string_body.extend_from_slice(&(key.len() as u32).to_le_bytes());
        for unit in key.encode_utf16() {
            string_body.extend_from_slice(&unit.to_le_bytes());
        }
        inflated.extend_from_slice(&(21_u32 + string_body.len() as u32).to_le_bytes());
        inflated.extend_from_slice(&[
            0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb,
            0x59, 0x97,
        ]);
        inflated.push(5);
        inflated.extend_from_slice(&4_u32.to_le_bytes());
        inflated.extend_from_slice(&string_body);
        inflated.extend_from_slice(&16_u32.to_le_bytes());
        inflated.extend_from_slice(&[0xff; 16]);
        inflated.extend_from_slice(&1_u16.to_le_bytes());
        inflated.extend_from_slice(&1_u32.to_le_bytes());
        inflated.extend_from_slice(&2_u32.to_le_bytes());
        inflated.extend_from_slice(&4_u32.to_le_bytes());
        inflated.extend_from_slice(&3_u32.to_le_bytes());
        inflated.extend_from_slice(&0_u32.to_le_bytes());

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&inflated).unwrap();
        let compressed = encoder.finish().unwrap();
        let mut data = vec![0; 33];
        data.extend_from_slice(&compressed);
        let container = Container {
            data,
            version: 6,
            file_tag: 0,
            footer_offset: 0,
            entries: Vec::new(),
        };
        let scene = DisplayJtSegment {
            id: "scene".into(),
            document: "document".into(),
            toc_entry: "scene-entry".into(),
            segment_id: vec![1; 16],
            segment_type: 1,
            segment_byte_len: (33 + compressed.len()) as u32,
            payload_sha256: String::new(),
            compression: None,
            source_offset: 0,
        };
        let shape = DisplayJtSegment {
            id: "shape".into(),
            document: "document".into(),
            toc_entry: "shape-entry".into(),
            segment_id: vec![9; 16],
            segment_type: 7,
            segment_byte_len: 0,
            payload_sha256: String::new(),
            compression: None,
            source_offset: 0,
        };
        let bindings = super::display_jt_shape_lod_bindings(&container, &[scene, shape]);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].shape_node_object_id, 2);
        assert_eq!(bindings[0].shape_segment, "shape");
        assert_eq!(bindings[0].payload_object_id, 12);
        assert_eq!(bindings[0].key, key);
    }

    #[test]
    fn display_jt_string_property_body_requires_exact_utf16_frame() {
        let mut body = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
        body.extend_from_slice(&3_u32.to_le_bytes());
        body.extend_from_slice(&[b'N', 0, b'X', 0, 0xa9, 0x03]);
        let (units, value) = super::parse_jt_string_property_atom_body(&body).unwrap();
        assert_eq!(units, [0x4e, 0x58, 0x3a9]);
        assert_eq!(value, "NXΩ");

        body.push(0);
        assert!(super::parse_jt_string_property_atom_body(&body).is_none());
    }

    #[test]
    fn display_jt9_tri_strip_header_requires_supported_versions() {
        let mut body = Vec::new();
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&0x4a_u64.to_le_bytes());
        body.extend_from_slice(&2_u16.to_le_bytes());
        body.extend_from_slice(&0x1234_u32.to_le_bytes());
        body.extend_from_slice(&2_u16.to_le_bytes());
        body.extend_from_slice(&[9, 8, 7]);
        let (bindings, mesh_version, records_id, compressed_version, compressed) =
            super::parse_jt9_tri_strip_lod_header(&body).unwrap();
        assert_eq!(bindings, 0x4a);
        assert_eq!(mesh_version, 2);
        assert_eq!(records_id, 0x1234);
        assert_eq!(compressed_version, 2);
        assert_eq!(compressed, [9, 8, 7]);

        body[12..14].copy_from_slice(&3_u16.to_le_bytes());
        assert!(super::parse_jt9_tri_strip_lod_header(&body).is_none());
    }

    #[test]
    fn jt_scene_binding_transfers_visible_triangles_in_document_units() {
        use super::{
            DisplayJtBaseNodeData, DisplayJtCompressedElement,
            DisplayJtCompressedVertexRecordsHeader, DisplayJtGeometricTransformAttribute,
            DisplayJtGroupNodeData, DisplayJtInstanceNode, DisplayJtPolygonMesh,
            DisplayJtShapeLodBinding, DisplayJtShapeLodElement, DisplayJtTriStripShapeNode,
            DisplayJtVertexColors, DisplayJtVertexCoordinateArrayHeader,
            DisplayJtVertexCoordinates, DisplayJtVertexFlags, DisplayJtVertexNormals,
            DisplayJtVertexTextureCoordinates,
        };

        let mesh = DisplayJtPolygonMesh {
            id: "native-mesh".into(),
            topology: "topology".into(),
            coordinate_header: "coordinate-header".into(),
            polygons: vec![vec![0, 1, 2], vec![2, 1, 0, 2]],
            vertex_attribute_indices: vec![vec![Some(0), Some(1), Some(2)], vec![None; 4]],
            polygon_groups: vec![4, -1],
            polygon_flags: vec![0, 0],
            source_offset: 80,
        };
        let coordinates = DisplayJtVertexCoordinates {
            id: "coordinates".into(),
            header: "coordinate-header".into(),
            points_m: vec![[0.0, 0.0, 0.0], [0.001, 0.0, 0.0], [0.0, 0.002, 0.0]],
            coordinate_hash: 0,
            byte_len: 4,
            source_offset: 90,
        };
        let header = DisplayJtVertexCoordinateArrayHeader {
            id: "coordinate-header".into(),
            element: "shape-element".into(),
            unique_vertex_count: 3,
            component_count: 3,
            component_ranges: [[0.0, 0.0]; 3],
            component_quantization_bits: [0; 3],
            compressed_components_byte_len: 4,
            compressed_components_sha256: "00".repeat(32),
            source_offset: 60,
        };
        let shape_element = DisplayJtShapeLodElement {
            id: "shape-element".into(),
            segment: "shape-segment".into(),
            ordinal: 0,
            object_type_id: vec![0; 16],
            object_base_type: 4,
            object_id: 7,
            body_byte_len: 0,
            body_sha256: "00".repeat(32),
            source_offset: 100,
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
            source_offset: 110,
        };
        let base = DisplayJtBaseNodeData {
            id: "base".into(),
            element: "scene-element".into(),
            object_type_id: vec![0; 16],
            object_id: 9,
            version: 1,
            flags: 0,
            attribute_object_ids: vec![10],
            family_data_byte_len: 0,
            family_data_sha256: "00".repeat(32),
            source_offset: 120,
        };
        let compressed = DisplayJtCompressedElement {
            id: "scene-element".into(),
            segment: "scene-segment".into(),
            segment_type: 1,
            ordinal: 0,
            object_type_id: vec![0; 16],
            object_base_type: 2,
            object_id: 9,
            body_byte_len: 0,
            body_sha256: "00".repeat(32),
            inflated_offset: 0,
            source_offset: 120,
        };
        let instance_base = DisplayJtBaseNodeData {
            id: "instance-base".into(),
            element: "instance-element".into(),
            object_type_id: vec![0; 16],
            object_id: 11,
            version: 1,
            flags: 0,
            attribute_object_ids: Vec::new(),
            family_data_byte_len: 6,
            family_data_sha256: "00".repeat(32),
            source_offset: 122,
        };
        let instance_element = DisplayJtCompressedElement {
            id: "instance-element".into(),
            segment: "scene-segment".into(),
            segment_type: 1,
            ordinal: 1,
            object_type_id: vec![0; 16],
            object_base_type: 0,
            object_id: 11,
            body_byte_len: 0,
            body_sha256: "00".repeat(32),
            inflated_offset: 0,
            source_offset: 122,
        };
        let instance = DisplayJtInstanceNode {
            id: "instance-node".into(),
            base_node: "instance-base".into(),
            object_id: 11,
            version: 1,
            child_object_id: 9,
            source_offset: 122,
        };
        let mut second_instance_base = instance_base.clone();
        second_instance_base.id = "second-instance-base".into();
        second_instance_base.element = "second-instance-element".into();
        second_instance_base.object_id = 12;
        second_instance_base.source_offset = 123;
        let mut second_instance_element = instance_element.clone();
        second_instance_element.id = "second-instance-element".into();
        second_instance_element.ordinal = 2;
        second_instance_element.object_id = 12;
        second_instance_element.source_offset = 123;
        let second_instance = DisplayJtInstanceNode {
            id: "second-instance-node".into(),
            base_node: "second-instance-base".into(),
            object_id: 12,
            version: 1,
            child_object_id: 9,
            source_offset: 123,
        };
        let group_base = DisplayJtBaseNodeData {
            id: "group-base".into(),
            element: "group-element".into(),
            object_type_id: vec![0; 16],
            object_id: 20,
            version: 1,
            flags: 0,
            attribute_object_ids: Vec::new(),
            family_data_byte_len: 14,
            family_data_sha256: "00".repeat(32),
            source_offset: 124,
        };
        let group_element = DisplayJtCompressedElement {
            id: "group-element".into(),
            segment: "scene-segment".into(),
            segment_type: 1,
            ordinal: 3,
            object_type_id: vec![0; 16],
            object_base_type: 1,
            object_id: 20,
            body_byte_len: 0,
            body_sha256: "00".repeat(32),
            inflated_offset: 0,
            source_offset: 124,
        };
        let group = DisplayJtGroupNodeData {
            id: "group-node".into(),
            base_node: "group-base".into(),
            object_id: 20,
            version: 1,
            child_object_ids: vec![11, 12],
            family_data_byte_len: 0,
            family_data_sha256: "00".repeat(32),
            source_offset: 124,
        };
        let mut ignored_group_base = group_base.clone();
        ignored_group_base.id = "ignored-group-base".into();
        ignored_group_base.element = "ignored-group-element".into();
        ignored_group_base.object_id = 21;
        ignored_group_base.flags = 1;
        ignored_group_base.source_offset = 125;
        let mut ignored_group_element = group_element.clone();
        ignored_group_element.id = "ignored-group-element".into();
        ignored_group_element.ordinal = 4;
        ignored_group_element.object_id = 21;
        ignored_group_element.source_offset = 125;
        let ignored_group = DisplayJtGroupNodeData {
            id: "ignored-group-node".into(),
            base_node: "ignored-group-base".into(),
            object_id: 21,
            version: 1,
            child_object_ids: vec![9],
            family_data_byte_len: 0,
            family_data_sha256: "00".repeat(32),
            source_offset: 125,
        };
        let transform = DisplayJtGeometricTransformAttribute {
            id: "transform".into(),
            element: "scene-element".into(),
            object_id: 10,
            state_flags: 0,
            field_inhibit_flags: 0,
            stored_values_mask: 0xffff,
            matrix: [
                [2.0, 0.0, 0.0, 0.0],
                [0.0, 3.0, 0.0, 0.0],
                [0.0, 0.0, 4.0, 0.0],
                [0.01, 0.02, 0.03, 1.0],
            ],
            source_offset: 121,
        };
        let node = DisplayJtTriStripShapeNode {
            id: "shape-node".into(),
            base_node: "base".into(),
            object_id: 9,
            reserved_bounds: [[0.0; 3]; 2],
            untransformed_bounds: [[0.0; 3]; 2],
            area: 0.0,
            vertex_count_range: [0, 0],
            node_count_range: [0, 0],
            polygon_count_range: [0, 0],
            memory_byte_len: 0,
            compression_level: 0.0,
            vertex_version: 1,
            vertex_bindings: 2,
            vertex_quantization_bits: 0,
            normal_quantization_factor: 0,
            texture_quantization_bits: 0,
            color_quantization_bits: 0,
            version_2_vertex_bindings: None,
            source_offset: 120,
        };
        let vertex_header = DisplayJtCompressedVertexRecordsHeader {
            id: "vertex-header".into(),
            element: "shape-element".into(),
            vertex_bindings: 0x15a,
            vertex_quantization_bits: 0,
            normal_quantization_factor: 0,
            texture_quantization_bits: 0,
            color_quantization_bits: 0,
            topological_vertex_count: 3,
            vertex_attribute_count: 3,
            compressed_arrays_byte_len: 0,
            compressed_arrays_sha256: "00".repeat(32),
            source_offset: 80,
        };
        let normals = DisplayJtVertexNormals {
            id: "normals".into(),
            vertex_records_header: "vertex-header".into(),
            normals: vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            normal_hash: 0,
            byte_len: 4,
            source_offset: 94,
        };
        let colors = DisplayJtVertexColors {
            id: "colors".into(),
            vertex_records_header: "vertex-header".into(),
            colors: vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 0.5],
                [0.0, 0.0, 1.0, 0.25],
            ],
            color_hash: 0,
            byte_len: 4,
            source_offset: 98,
        };
        let texture_coordinates = DisplayJtVertexTextureCoordinates {
            id: "texture".into(),
            vertex_records_header: "vertex-header".into(),
            channel: 0,
            values: vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![0.0, 1.0]],
            texture_coordinate_hash: 0,
            byte_len: 4,
            source_offset: 102,
        };
        let vertex_flags = DisplayJtVertexFlags {
            id: "flags".into(),
            vertex_records_header: "vertex-header".into(),
            values: vec![0, 1, 0],
            byte_len: 4,
            source_offset: 106,
        };

        let tessellations = super::display_jt_tessellations(&super::DisplayJtTessellationInputs {
            meshes: &[mesh],
            coordinates: &[coordinates],
            normals: &[normals],
            colors: &[colors],
            texture_coordinates: &[texture_coordinates],
            vertex_flags: &[vertex_flags],
            vertex_headers: &[vertex_header],
            coordinate_headers: &[header],
            shape_elements: &[shape_element],
            bindings: &[binding],
            shape_nodes: &[node],
            base_nodes: &[
                base,
                instance_base,
                second_instance_base,
                group_base,
                ignored_group_base,
            ],
            group_nodes: &[group, ignored_group],
            instance_nodes: &[instance, second_instance],
            transforms: &[transform],
            compressed_elements: &[
                compressed,
                instance_element,
                second_instance_element,
                group_element,
                ignored_group_element,
            ],
        })
        .expect("complete scene binding");
        assert_eq!(tessellations.len(), 2);
        assert!((tessellations[0].0.vertices[1].x - 12.0).abs() < 1e-6);
        assert!((tessellations[0].0.vertices[2].y - 26.0).abs() < 1e-6);
        assert_eq!(tessellations[0].0.triangles, vec![[0, 1, 2]]);
        assert_eq!(
            tessellations[0].0.normals[1],
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
        );
        assert_eq!(
            tessellations[0].0.source_object.as_ref().unwrap().object_id,
            "shape-node"
        );
        assert_eq!(
            tessellations[0]
                .0
                .source_object
                .as_ref()
                .unwrap()
                .instance_path,
            ["instance-node"]
        );
        assert_eq!(
            tessellations[1]
                .0
                .source_object
                .as_ref()
                .unwrap()
                .instance_path,
            ["second-instance-node"]
        );
        assert_eq!(tessellations[0].0.channels.len(), 3);
        assert_eq!(tessellations[0].0.channels[0].kind, 0x4e58_0001);
        assert_eq!(tessellations[0].0.channels[0].item_size, 16);
        assert_eq!(tessellations[0].0.channels[0].flags, 1);
        assert_eq!(tessellations[0].0.channels[0].count, 3);
        assert_eq!(
            &tessellations[0].0.channels[0].data[16..32],
            &[
                0.0_f32.to_le_bytes(),
                1.0_f32.to_le_bytes(),
                0.0_f32.to_le_bytes(),
                0.5_f32.to_le_bytes(),
            ]
            .concat()
        );
        assert_eq!(tessellations[0].0.channels[1].kind, 0x4e58_0100);
        assert_eq!(tessellations[0].0.channels[1].item_size, 8);
        assert_eq!(tessellations[0].0.channels[1].flags, 0x100);
        assert_eq!(tessellations[0].0.channels[1].count, 3);
        assert_eq!(
            &tessellations[0].0.channels[1].data[16..24],
            &[0.0_f32.to_le_bytes(), 1.0_f32.to_le_bytes()].concat()
        );
        assert_eq!(tessellations[0].0.channels[2].kind, 0x4e58_0002);
        assert_eq!(tessellations[0].0.channels[2].item_size, 4);
        assert_eq!(tessellations[0].0.channels[2].count, 3);
        assert_eq!(
            tessellations[0].0.channels[2].data,
            [
                0_u32.to_le_bytes(),
                1_u32.to_le_bytes(),
                0_u32.to_le_bytes(),
            ]
            .concat()
        );
        assert_eq!(tessellations[0].1, 120);
    }

    #[test]
    fn jt9_topology_bounds_variable_high_degree_lane_count() {
        fn representation(high_degree_lanes: usize, topological_vertices: u32) -> Vec<u8> {
            let mut bytes = vec![0; (21 + high_degree_lanes + 2) * 4];
            bytes.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
            bytes.extend_from_slice(&10_u64.to_le_bytes());
            bytes.extend_from_slice(&[24, 13, 16, 8]);
            bytes.extend_from_slice(&topological_vertices.to_le_bytes());
            if topological_vertices != 0 {
                bytes.extend_from_slice(&(topological_vertices + 1).to_le_bytes());
            }
            bytes
        }

        let empty = representation(1, 0);
        assert_eq!(
            super::jt9_topology_high_degree_lane_count(&empty, 10),
            Some(1)
        );
        let populated = representation(13, 20);
        assert_eq!(
            super::jt9_topology_high_degree_lane_count(&populated, 10),
            Some(13)
        );
        assert_eq!(
            super::jt9_topology_high_degree_lane_count(&populated, 11),
            None
        );
    }

    #[test]
    fn display_jt_base_node_body_bounds_ordered_attribute_ids() {
        let mut body = Vec::new();
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&0x20_u32.to_le_bytes());
        body.extend_from_slice(&2_u32.to_le_bytes());
        body.extend_from_slice(&7_u32.to_le_bytes());
        body.extend_from_slice(&9_u32.to_le_bytes());
        body.extend_from_slice(&[4, 3, 2, 1]);
        let (version, flags, attributes, family) =
            super::parse_jt_base_node_body(&body, 9).unwrap();
        assert_eq!(version, 1);
        assert_eq!(flags, 0x20);
        assert_eq!(attributes, [7, 9]);
        assert_eq!(family, [4, 3, 2, 1]);

        body.truncate(17);
        assert!(super::parse_jt_base_node_body(&body, 9).is_none());

        let mut modern = vec![2];
        modern.extend_from_slice(&0x40_u32.to_le_bytes());
        modern.extend_from_slice(&1_u32.to_le_bytes());
        modern.extend_from_slice(&11_u32.to_le_bytes());
        modern.push(0xaa);
        let (version, flags, attributes, family) =
            super::parse_jt_base_node_body(&modern, 10).unwrap();
        assert_eq!((version, flags), (2, 0x40));
        assert_eq!(attributes, [11]);
        assert_eq!(family, [0xaa]);
    }

    #[test]
    fn display_jt9_instance_node_requires_one_exact_child_reference() {
        let mut body = Vec::new();
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&0x20_u32.to_le_bytes());
        body.extend_from_slice(&1_u32.to_le_bytes());
        body.extend_from_slice(&7_u32.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&9_u32.to_le_bytes());

        assert_eq!(super::parse_jt9_instance_node_body(&body), Some((1, 9)));
        body.push(0);
        assert!(super::parse_jt9_instance_node_body(&body).is_none());
        body.pop();
        body[14..16].copy_from_slice(&2_u16.to_le_bytes());
        assert!(super::parse_jt9_instance_node_body(&body).is_none());
    }

    #[test]
    fn display_jt9_group_node_bounds_ordered_children_and_family_tail() {
        let mut body = Vec::new();
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&2_u32.to_le_bytes());
        body.extend_from_slice(&7_u32.to_le_bytes());
        body.extend_from_slice(&9_u32.to_le_bytes());
        body.extend_from_slice(&[4, 3, 2, 1]);

        let (version, children, family) = super::parse_jt9_group_node_body(&body).unwrap();
        assert_eq!(version, 1);
        assert_eq!(children, [7, 9]);
        assert_eq!(family, [4, 3, 2, 1]);
        body.truncate(body.len() - 5);
        assert!(super::parse_jt9_group_node_body(&body).is_none());
    }

    #[test]
    fn display_jt9_tri_strip_shape_node_requires_exact_shape_data() {
        let mut body = Vec::new();
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&0x20_u32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        for value in [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        for value in [-3.0_f32, -2.0, -1.0, 0.0, 1.0, 2.0] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        body.extend_from_slice(&6.0_f32.to_le_bytes());
        for value in [7_i32, 8, 9, 10, 11, 12] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        body.extend_from_slice(&4096_u32.to_le_bytes());
        body.extend_from_slice(&0.75_f32.to_le_bytes());
        body.extend_from_slice(&2_u16.to_le_bytes());
        body.extend_from_slice(&0x102_u64.to_le_bytes());
        body.extend_from_slice(&[24, 13, 16, 8]);
        body.extend_from_slice(&0x304_u64.to_le_bytes());

        let node = super::parse_jt9_tri_strip_shape_node_body(&body).unwrap();
        assert_eq!(node.reserved_bounds, [[0.0, 1.0, 2.0], [3.0, 4.0, 5.0]]);
        assert_eq!(
            node.untransformed_bounds,
            [[-3.0, -2.0, -1.0], [0.0, 1.0, 2.0]]
        );
        assert_eq!(node.area, 6.0);
        assert_eq!(node.vertex_count_range, [7, 8]);
        assert_eq!(node.node_count_range, [9, 10]);
        assert_eq!(node.polygon_count_range, [11, 12]);
        assert_eq!(node.memory_byte_len, 4096);
        assert_eq!(node.compression_level, 0.75);
        assert_eq!(node.vertex_version, 2);
        assert_eq!(node.vertex_bindings, 0x102);
        assert_eq!(node.vertex_quantization_bits, 24);
        assert_eq!(node.normal_quantization_factor, 13);
        assert_eq!(node.texture_quantization_bits, 16);
        assert_eq!(node.color_quantization_bits, 8);
        assert_eq!(node.version_2_vertex_bindings, Some(0x304));

        let mut malformed = body.clone();
        malformed[60..64].copy_from_slice(&(-1.0_f32).to_le_bytes());
        assert!(super::parse_jt9_tri_strip_shape_node_body(&malformed).is_none());
        let mut malformed = body.clone();
        malformed[109] = 25;
        assert!(super::parse_jt9_tri_strip_shape_node_body(&malformed).is_none());
        body.truncate(body.len() - 8);
        assert!(super::parse_jt9_tri_strip_shape_node_body(&body).is_none());
    }

    #[test]
    fn display_jt9_geometric_transform_reconstructs_sparse_affine_matrix() {
        let mut body = 1_u16.to_le_bytes().to_vec();
        body.push(0x08);
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&0x000e_u16.to_le_bytes());
        for value in [1.25_f32, -2.5, 4.0] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        let (state, inhibit, mask, matrix) =
            super::parse_jt9_geometric_transform_body(&body).unwrap();
        assert_eq!(state, 0x08);
        assert_eq!(inhibit, 0);
        assert_eq!(mask, 0x000e);
        assert_eq!(matrix[0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(matrix[3], [1.25, -2.5, 4.0, 1.0]);

        body[2] = 0x10;
        assert!(super::parse_jt9_geometric_transform_body(&body).is_none());

        let mut shear = 1_u16.to_le_bytes().to_vec();
        shear.push(0);
        shear.extend_from_slice(&0_u32.to_le_bytes());
        shear.extend_from_slice(&1_u16.to_le_bytes());
        shear.extend_from_slice(&0x4800_u16.to_le_bytes());
        shear.extend_from_slice(&0.5_f32.to_le_bytes());
        shear.extend_from_slice(&0.5_f32.to_le_bytes());
        assert!(super::parse_jt9_geometric_transform_body(&shear).is_none());
    }

    #[test]
    fn display_jt9_partition_node_requires_complete_bounds_and_ranges() {
        let mut body = Vec::new();
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&1_u32.to_le_bytes());
        body.extend_from_slice(&2_u32.to_le_bytes());
        body.extend_from_slice(&1_u32.to_le_bytes());
        body.extend_from_slice(&1_u32.to_le_bytes());
        body.extend_from_slice(&u16::from(b'x').to_le_bytes());
        for value in [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        body.extend_from_slice(&6.0_f32.to_le_bytes());
        for value in [1_i32, 2, 3, 4, 5, 6] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        for value in [-3.0_f32, -2.0, -1.0, 0.0, 1.0, 2.0] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        let node = super::parse_jt9_partition_node_body(&body).unwrap();
        assert_eq!(node.group_version, 1);
        assert_eq!(node.child_object_ids, [2]);
        assert_eq!(node.file_name, "x");
        assert_eq!(node.transformed_bounds, [[0.0, 1.0, 2.0], [3.0, 4.0, 5.0]]);
        assert_eq!(node.area, 6.0);
        assert_eq!(node.vertex_count_range, [1, 2]);
        assert_eq!(node.node_count_range, [3, 4]);
        assert_eq!(node.polygon_count_range, [5, 6]);
        assert_eq!(
            node.untransformed_bounds,
            Some([[-3.0, -2.0, -1.0], [0.0, 1.0, 2.0]])
        );
        assert!(node.reserved_bounds.is_none());

        body.pop();
        assert!(super::parse_jt9_partition_node_body(&body).is_none());
    }

    #[test]
    fn display_jt9_range_lod_requires_ordered_finite_limits() {
        let mut body = Vec::new();
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&2_u32.to_le_bytes());
        body.extend_from_slice(&7_u32.to_le_bytes());
        body.extend_from_slice(&9_u32.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&1_u32.to_le_bytes());
        body.extend_from_slice(&0.25_f32.to_le_bytes());
        body.extend_from_slice(&(-2_i32).to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&2_u32.to_le_bytes());
        body.extend_from_slice(&10.0_f32.to_le_bytes());
        body.extend_from_slice(&20.0_f32.to_le_bytes());
        for value in [1.0_f32, 2.0, 3.0] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        let node = super::parse_jt9_range_lod_node_body(&body).unwrap();
        assert_eq!(node.group_version, 1);
        assert_eq!(node.child_object_ids, [7, 9]);
        assert_eq!(node.lod_version, 1);
        assert_eq!(node.reserved_values, [0.25]);
        assert_eq!(node.reserved_value, -2);
        assert_eq!(node.range_version, 1);
        assert_eq!(node.range_limits, [10.0, 20.0]);
        assert_eq!(node.center, [1.0, 2.0, 3.0]);

        let range_offset = body.len() - 20;
        body[range_offset..range_offset + 4].copy_from_slice(&5.0_f32.to_le_bytes());
        body[range_offset + 4..range_offset + 8].copy_from_slice(&4.0_f32.to_le_bytes());
        assert!(super::parse_jt9_range_lod_node_body(&body).is_none());
    }

    #[test]
    fn jt9_topology_packets_retain_decoded_primal_values() {
        use super::{display_jt_topology_packet_sequences, DisplayJtShapeLodElement};

        let mut representation = vec![0; 24 * 4];
        representation.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
        representation.extend_from_slice(&10_u64.to_le_bytes());
        representation.extend_from_slice(&[24, 13, 16, 8]);
        representation.extend_from_slice(&0_u32.to_le_bytes());

        let mut body = Vec::new();
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&10_u64.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&7_u32.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&representation);
        let source_offset = 64_u64;
        let mut data = vec![0; source_offset as usize + 25];
        data.extend_from_slice(&body);
        let container = crate::container::Container {
            data,
            version: 1,
            file_tag: 0,
            footer_offset: 0,
            entries: Vec::new(),
        };
        let elements = [DisplayJtShapeLodElement {
            id: "shape-lod".into(),
            segment: "segment".into(),
            ordinal: 0,
            object_type_id: vec![
                0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb,
                0x59, 0x97,
            ],
            object_base_type: 4,
            object_id: 1,
            body_byte_len: body.len() as u32,
            body_sha256: String::new(),
            source_offset,
        }];

        let (sequences, _, _) = display_jt_topology_packet_sequences(&container, &elements);
        assert_eq!(sequences.len(), 1);
        assert_eq!(sequences[0].packets.len(), 24);
        assert!(sequences[0]
            .packets
            .iter()
            .all(|packet| packet.values == Some(Vec::new())));
    }
}
