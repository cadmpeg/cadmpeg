// SPDX-License-Identifier: Apache-2.0
//! Typed Siemens NX object-model records retained in the native namespace.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;

use flate2::read::ZlibDecoder;
use serde::{Deserialize, Serialize};

use cadmpeg_ir::hash::sha256_hex;

use crate::container::Container;
use crate::parasolid::{Stream, StreamKind};

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

/// Complete typed source record for one Parasolid offset surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidOffsetSurfaceRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the offset surface.
    pub xmt: u32,
    /// Serialized `V`, `I`, or `U` discriminator.
    pub discriminator: char,
    /// Serialized true-offset flag.
    pub true_offset: bool,
    /// Cross-reference index of the support surface.
    pub support_xmt: u32,
    /// Signed offset distance in millimetres.
    pub distance: f64,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid offset surfaces.
pub fn parasolid_offset_surface_records(streams: &[Stream]) -> Vec<ParasolidOffsetSurfaceRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for offset in crate::topology::offset_surfaces(&stream.inflated) {
            records.push(ParasolidOffsetSurfaceRecord {
                id: format!("nx:s{stream_ordinal}:offset-surface-record#{}", offset.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: offset.xmt,
                discriminator: offset.discriminator,
                true_offset: offset.true_offset,
                support_xmt: offset.support,
                distance: offset.distance,
                inflated_offset: offset.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid trimmed curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidTrimmedCurveRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the trimmed curve.
    pub xmt: u32,
    /// Cross-reference index of the basis curve.
    pub basis_xmt: u32,
    /// Stored start and end points in millimetres.
    pub points: [[f64; 3]; 2],
    /// Stored start and end parameters in basis-curve units.
    pub parameters: [f64; 2],
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid trimmed curves.
pub fn parasolid_trimmed_curve_records(streams: &[Stream]) -> Vec<ParasolidTrimmedCurveRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for trim in crate::topology::trimmed_curves(&stream.inflated) {
            records.push(ParasolidTrimmedCurveRecord {
                id: format!("nx:s{stream_ordinal}:trimmed-curve-record#{}", trim.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: trim.xmt,
                basis_xmt: trim.basis,
                points: trim.points,
                parameters: trim.parameters,
                inflated_offset: trim.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid surface curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidSurfaceCurveRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the surface curve.
    pub xmt: u32,
    /// Cross-reference index of the support surface.
    pub surface_xmt: u32,
    /// Cross-reference index of the parameter-space B-curve.
    pub pcurve_xmt: u32,
    /// Nullable cross-reference index of the original model-space curve.
    pub original_curve_xmt: u32,
    /// Serialized tolerance to the original curve in Parasolid metres.
    pub tolerance_to_original: f64,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid surface curves.
pub fn parasolid_surface_curve_records(streams: &[Stream]) -> Vec<ParasolidSurfaceCurveRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for curve in crate::topology::surface_curves(&stream.inflated) {
            records.push(ParasolidSurfaceCurveRecord {
                id: format!("nx:s{stream_ordinal}:surface-curve-record#{}", curve.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: curve.xmt,
                surface_xmt: curve.surface,
                pcurve_xmt: curve.pcurve,
                original_curve_xmt: curve.original,
                tolerance_to_original: curve.tolerance,
                inflated_offset: curve.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid blend-bound bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidBlendBoundRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the bridge.
    pub xmt: u32,
    /// Five ordered common-header references.
    pub header_references: [u32; 5],
    /// Serialized orientation sense.
    pub sense: bool,
    /// Zero- or one-valued blend boundary index.
    pub boundary_index: u32,
    /// Cross-reference index of the blend surface.
    pub blend_surface_xmt: u32,
    /// Whether the record tag uses the `0xff` envelope escape.
    pub escaped: bool,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid blend-bound bridges.
pub fn parasolid_blend_bound_records(streams: &[Stream]) -> Vec<ParasolidBlendBoundRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for bound in crate::intersection::blend_bounds(&stream.inflated) {
            records.push(ParasolidBlendBoundRecord {
                id: format!("nx:s{stream_ordinal}:blend-bound-record#{}", bound.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: bound.xmt,
                header_references: bound.header_references,
                sense: bound.sense,
                boundary_index: bound.boundary_index,
                blend_surface_xmt: bound.blend_surface,
                escaped: bound.escaped,
                inflated_offset: bound.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Serialized framing of a Parasolid `term_use` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidTermUseFraming {
    /// Direct `0x0029` tag.
    Direct,
    /// `0x0029ff` escaped tag.
    Escaped,
    /// Payload following the inline descriptor.
    DescriptorInline,
}

/// Complete typed source record for one Parasolid `term_use` endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidTermUseRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the endpoint.
    pub xmt: u32,
    /// Serialized leading count.
    pub count: u32,
    /// Two-byte endpoint-form discriminator as printable ASCII.
    pub form: String,
    /// Endpoint position in millimetres.
    pub point: [f64; 3],
    /// Serialized record framing.
    pub framing: ParasolidTermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid `term_use` endpoints.
pub fn parasolid_term_use_records(streams: &[Stream]) -> Vec<ParasolidTermUseRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for term in crate::intersection::term_use_records(&stream.inflated) {
            let framing = match term.framing {
                crate::intersection::TermUseFraming::Direct => ParasolidTermUseFraming::Direct,
                crate::intersection::TermUseFraming::Escaped => ParasolidTermUseFraming::Escaped,
                crate::intersection::TermUseFraming::DescriptorInline => {
                    ParasolidTermUseFraming::DescriptorInline
                }
            };
            records.push(ParasolidTermUseRecord {
                id: format!("nx:s{stream_ordinal}:term-use-record#{}", term.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: term.xmt,
                count: term.count,
                form: String::from_utf8_lossy(&term.form).into_owned(),
                point: [term.point.x, term.point.y, term.point.z],
                framing,
                inflated_offset: term.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Serialized framing of a Parasolid support-UV values array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidSupportUvFraming {
    /// Direct `0x00cc` tag.
    Direct,
    /// `0x00ccff` escaped tag.
    Escaped,
    /// Payload following the inline descriptor.
    DescriptorInline,
}

/// Complete typed source record for one Parasolid support-UV values array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidSupportUvRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the values array.
    pub xmt: u32,
    /// Serialized scalar count.
    pub count: u32,
    /// Tuple-packing marker (`2`, `3`, or `4`).
    pub marker: u8,
    /// Ordered serialized scalar values.
    pub values: Vec<f64>,
    /// Serialized record framing.
    pub framing: ParasolidSupportUvFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid support-UV arrays.
pub fn parasolid_support_uv_records(streams: &[Stream]) -> Vec<ParasolidSupportUvRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for record in crate::intersection::support_uv_records(&stream.inflated) {
            let framing = match record.framing {
                crate::intersection::SupportUvFraming::Direct => ParasolidSupportUvFraming::Direct,
                crate::intersection::SupportUvFraming::Escaped => {
                    ParasolidSupportUvFraming::Escaped
                }
                crate::intersection::SupportUvFraming::DescriptorInline => {
                    ParasolidSupportUvFraming::DescriptorInline
                }
            };
            records.push(ParasolidSupportUvRecord {
                id: format!("nx:s{stream_ordinal}:support-uv-record#{}", record.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                count: record.count,
                marker: record.marker,
                values: record.values,
                framing,
                inflated_offset: record.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Hvec point layout of a Parasolid chart record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidChartPointLayout {
    /// Three model-space coordinates per point.
    Xyz3,
    /// Eleven scalars containing point, UV lanes, tangent, and parameter.
    Ext11,
}

/// Serialized framing of a Parasolid chart record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidChartFraming {
    /// Direct `0x0028` tag.
    Direct,
    /// `0x0028ff` escaped tag.
    Escaped,
}

/// Complete typed source record for one physical Parasolid `CHART_s` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidChartRecord {
    /// Globally unique physical-record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the chart.
    pub xmt: u32,
    /// Serialized leading point count.
    pub count: u32,
    /// Base chart parameter.
    pub base_parameter: f64,
    /// Chord-to-parameter scale.
    pub base_scale: f64,
    /// Redundant serialized chart count.
    pub chart_count: u32,
    /// Chordal error in Parasolid metres.
    pub chordal_error: f64,
    /// Angular error in radians.
    pub angular_error: f64,
    /// Two serialized missing-parameter sentinels.
    pub parameter_errors: [f64; 2],
    /// Model-space chart points in millimetres.
    pub points: Vec<[f64; 3]>,
    /// Native ext11 parameters, when present.
    pub native_parameters: Option<Vec<f64>>,
    /// Two ordered ext11 support-UV lanes.
    pub ext_support_uv: [Option<Vec<[f64; 2]>>; 2],
    /// Hvec point layout.
    pub point_layout: ParasolidChartPointLayout,
    /// Serialized record framing.
    pub framing: ParasolidChartFraming,
    /// Type-tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode every complete physical Parasolid chart source record.
pub fn parasolid_chart_records(streams: &[Stream]) -> Vec<ParasolidChartRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for chart in crate::intersection::chart_source_records(&stream.inflated) {
            let point_layout = match chart.point_layout {
                crate::intersection::ChartPointLayout::Xyz3 => ParasolidChartPointLayout::Xyz3,
                crate::intersection::ChartPointLayout::Ext11 => ParasolidChartPointLayout::Ext11,
            };
            let framing = match chart.framing {
                crate::intersection::ChartFraming::Direct => ParasolidChartFraming::Direct,
                crate::intersection::ChartFraming::Escaped => ParasolidChartFraming::Escaped,
            };
            records.push(ParasolidChartRecord {
                id: format!(
                    "nx:s{stream_ordinal}:chart-record#{}-{}",
                    chart.xmt, chart.pos
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: chart.xmt,
                count: chart.count,
                base_parameter: chart.base_parameter,
                base_scale: chart.base_scale,
                chart_count: chart.chart_count,
                chordal_error: chart.chordal_error,
                angular_error: chart.angular_error,
                parameter_errors: chart.parameter_errors,
                points: chart
                    .points
                    .into_iter()
                    .map(|point| [point.x, point.y, point.z])
                    .collect(),
                native_parameters: chart.native_parameters,
                ext_support_uv: chart.ext_support_uv,
                point_layout,
                framing,
                inflated_offset: chart.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid surface-intersection curve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidIntersectionRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the construction.
    pub xmt: u32,
    /// Five ordered common-header references.
    pub header_references: [u32; 5],
    /// Serialized orientation sense.
    pub sense: bool,
    /// Six ordered support and witness references.
    pub construction_references: [u32; 6],
    /// Whether the record uses the single-byte delta-twin tag.
    pub delta_twin: bool,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for retained intersection constructions.
pub fn parasolid_intersection_records(streams: &[Stream]) -> Vec<ParasolidIntersectionRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for construction in crate::intersection::scan(&stream.inflated).constructions {
            records.push(ParasolidIntersectionRecord {
                id: format!(
                    "nx:s{stream_ordinal}:intersection-record#{}",
                    construction.xmt
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: construction.xmt,
                header_references: construction.header_references,
                sense: construction.sense,
                construction_references: construction.references,
                delta_twin: construction.delta_twin,
                inflated_offset: construction.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// One row retained from the canonical `UG_PART` segment index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based row ordinal.
    pub ordinal: u32,
    /// First little-endian row word.
    pub type_code: u32,
    /// Second little-endian row word.
    pub subtype_code: u32,
    /// Third little-endian row word.
    pub value: u32,
    /// Directory entry containing the index.
    pub source_entry: String,
    /// Absolute file offset of the row.
    pub source_offset: u64,
}

/// Decode the canonical `UG_PART` segment-index rows.
pub fn segment_index_rows(container: &Container) -> Vec<SegmentIndexRow> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    index
        .rows
        .into_iter()
        .enumerate()
        .map(|(ordinal, row)| SegmentIndexRow {
            id: format!("nx:segment-index:row#{ordinal}"),
            ordinal: ordinal as u32,
            type_code: row.type_code,
            subtype_code: row.subtype_code,
            value: row.value,
            source_entry: entry.name.clone(),
            source_offset: entry_offset + (ordinal * 12) as u64,
        })
        .collect()
}

/// Word position within one segment-index row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentIndexSlot {
    /// First row word.
    TypeCode,
    /// Second row word.
    SubtypeCode,
    /// Third row word.
    Value,
}

/// Validated link from a segment-index word to a compressed stream wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentStreamLink {
    /// Globally unique link identity.
    pub id: String,
    /// Owning segment-index row.
    pub row: String,
    /// Row word containing the wrapper offset.
    pub slot: SegmentIndexSlot,
    /// Zero-based stream ordinal in source-file order.
    pub stream_ordinal: u32,
    /// Decoded stream classification.
    pub stream_kind: String,
    /// Bytes from the wrapper start to its zlib header.
    pub wrapper_byte_len: u32,
    /// Absolute file offset of the wrapper.
    pub source_offset: u64,
}

/// Body-image identity carried beside one validated Parasolid stream wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentBodyBinding {
    /// Globally unique binding identity.
    pub id: String,
    /// Validated stream-wrapper link owning the metadata tuple.
    pub stream_link: String,
    /// Zero-based stream ordinal in source-file order.
    pub stream_ordinal: u32,
    /// Partition or plain cached-body stream classification.
    pub stream_kind: String,
    /// Object index used by feature-history body operands.
    pub body_object_index: u32,
    /// Second object index naming the same body image in feature history.
    pub body_alias_object_index: u32,
    /// Serialized role word completing the five-word segment tuple.
    pub stream_role: u32,
    /// Absolute file offset of the object-index word in the segment index.
    pub source_offset: u64,
}

/// Unambiguous terminal status of one segment-bound body image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentBodyLineageStatus {
    /// Globally unique status identity.
    pub id: String,
    /// Segment binding whose alias pair names the body image.
    pub segment_body_binding: String,
    /// First serialized body identity.
    pub body_object_index: u32,
    /// Alias identity naming the same body image.
    pub body_alias_object_index: u32,
    /// Whether the image remains terminal after retained history.
    pub terminal: bool,
    /// Absolute source offset of the segment binding.
    pub source_offset: u64,
}

/// Complete typed type-56 rolling-ball blend-surface record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidBlendSurfaceRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local `BLEND_SURF` identity.
    pub xmt: u32,
    /// Ordered support-surface identities.
    pub support_xmts: [u32; 2],
    /// Ball-centre spine identity; `1` is the null reference.
    pub spine_xmt: u32,
    /// Signed support offsets in model millimetres.
    pub offsets: [f64; 2],
    /// Dimensionless support thumb weights.
    pub thumb_weights: [f64; 2],
    /// Offset of the type tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Named Parasolid attribute class declared in one inflated body stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidAttributeDefinition {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local definition record identity.
    pub xmt: u16,
    /// Exact printable attribute class name.
    pub name: String,
    /// Declared number of fields.
    pub field_count: u32,
    /// Stream-local identity of the following field record.
    pub field_record_xmt: u16,
    /// Ordered catalog references in the field-record header.
    pub field_record_references: [u16; 2],
    /// Two field-record header words following the catalog references.
    pub field_record_header_words: [u16; 2],
    /// Exact 26-byte descriptor prefix following the field-record header.
    pub field_descriptor_prefix: [u8; 26],
    /// Typed primary storage declared by the descriptor's `03` atom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_storage: Option<ParasolidAttributeFieldStorage>,
    /// One serialized code for every declared field.
    pub field_codes: Vec<u8>,
    /// Offset of the declaration in the inflated stream.
    pub inflated_offset: u64,
}

/// Primary storage alphabet declared by a Parasolid attribute field descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidAttributeFieldStorage {
    /// Void or flag storage.
    Void,
    /// Component/reference or string storage.
    Component,
    /// Binary64 floating-point storage.
    Double,
}

pub(crate) fn parasolid_attribute_field_storage(
    descriptor: &[u8; 26],
) -> Option<ParasolidAttributeFieldStorage> {
    (descriptor[4] == 0x03).then_some(())?;
    match descriptor[5] {
        0x00 => Some(ParasolidAttributeFieldStorage::Void),
        0x05 => Some(ParasolidAttributeFieldStorage::Component),
        0x06 => Some(ParasolidAttributeFieldStorage::Double),
        _ => None,
    }
}

/// Explicit topology-record ownership of one Parasolid attribute list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidTopologyAttributeListReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Parasolid topology record type.
    pub topology_type: u8,
    /// Stream-local topology-record identity.
    pub topology_xmt: u32,
    /// Stream-local attribute-list identity.
    pub attribute_list_xmt: u32,
    /// Uniquely resolved type-81 attribute-list record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute_list_record: Option<String>,
    /// Offset of the attribute-list field in the inflated stream.
    pub inflated_offset: u64,
}

/// Framed Parasolid type-81 entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51Record {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Exact record flags.
    pub flags: u32,
    /// Serialized sequence value.
    pub sequence: u32,
    /// Layout discriminator.
    pub discriminator: u16,
    /// Ordered stream-local references.
    pub references: Vec<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Self-framed printable Parasolid type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity54StringRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Exact nonempty printable value.
    pub value: String,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-82 unsigned-integer record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity52IntegerRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered big-endian unsigned values.
    pub values: Vec<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-83 finite binary64 record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidEntity53DoubleRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered finite big-endian binary64 values.
    pub values: Vec<f64>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Numeric value-record family referenced by a type-81 record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidEntity51NumericKind {
    /// Type-82 unsigned-integer lane.
    UnsignedIntegers,
    /// Type-83 binary64 lane.
    Doubles,
}

/// Exact type-81 reference to one uniquely resolved numeric value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51NumericUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    pub reference_ordinal: u32,
    /// Stream-local referenced xmt.
    pub referenced_xmt: u32,
    /// Numeric record family.
    pub kind: ParasolidEntity51NumericKind,
    /// Uniquely resolved numeric record.
    pub value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Exact type-81 reference to a uniquely resolved type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51StringUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    pub reference_ordinal: u32,
    /// Stream-local referenced xmt.
    pub referenced_xmt: u32,
    /// Uniquely resolved type-84 string record.
    pub string_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Resolved registered class of one Parasolid type-81 attribute instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidAttributeClassUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Type-81 attribute-instance record.
    pub entity_51_record: String,
    /// Class discriminator serialized by the type-81 instance.
    pub class_discriminator: u16,
    /// Stream-local XMT of the matched type-79 definition.
    pub definition_xmt: u16,
    /// Uniquely matched attribute definition.
    pub attribute_definition: String,
}

/// Resolved class of one topology-owned Parasolid attribute instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidTopologyAttributeClassUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Owning topology-to-attribute relation.
    pub topology_attribute_reference: String,
    /// Topology-owned type-81 attribute-instance record.
    pub entity_51_record: String,
    /// Class discriminator serialized by the type-81 instance.
    pub class_discriminator: u16,
    /// Stream-local XMT of the matched type-79 definition.
    pub definition_xmt: u16,
    /// Uniquely matched attribute definition.
    pub attribute_definition: String,
}

/// Validated link from a segment-index word to a framed OM section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentOmLink {
    /// Globally unique link identity.
    pub id: String,
    /// Owning segment-index row.
    pub row: String,
    /// Row word containing the section offset.
    pub slot: SegmentIndexSlot,
    /// Role established by exact class declarations in the pointed registry.
    pub schema_role: OmSchemaRole,
    /// Bytes from the pointed offset to the OM section signature.
    pub separator_byte_len: u32,
    /// Absolute file offset of the pointed location.
    pub source_offset: u64,
    /// Absolute file offset of the `ff ff ff ff` OM signature.
    pub section_offset: u64,
}

/// Semantic family declared by a linked OM section's class registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmSchemaRole {
    /// General part object model declaring `UGS::Solid::Topol`.
    Model,
    /// Construction/history model declaring `UGS::FEATURE_RECORD`.
    FeatureHistory,
    /// Expression model declaring `UGS::EXP_expression`.
    Expressions,
    /// Audit model declaring `UGS::OM::SaveAuditTrail`.
    AuditTrail,
    /// No role marker from the defined families occurs in the registry.
    Other,
}

/// Internally pointed record area in a role-classified size-framed OM section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmRecordArea {
    /// Globally unique record-area identity.
    pub id: String,
    /// Link identifying the owning ordered OM section.
    pub section_link: String,
    /// Registry-derived role of the owning section.
    pub schema_role: OmSchemaRole,
    /// Three exact little-endian control words.
    pub control_words: [u32; 3],
    /// Exact printable product/version string.
    pub product_version: String,
    /// Exact record-area byte length.
    pub byte_len: u64,
    /// SHA-256 of the complete pointed record area.
    pub sha256: String,
    /// Absolute file offset of the first control word.
    pub source_offset: u64,
}

/// Ordered feature operation label from a feature-history record area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationLabel {
    /// Globally unique label identity.
    pub id: String,
    /// Link identifying the owning ordered OM section.
    pub section_link: String,
    /// Zero-based order within the record area.
    pub ordinal: u32,
    /// Exact printable operation name.
    pub value: String,
    /// Four object-index slots in header order.
    pub object_indices: [Option<u32>; 4],
    /// Absolute file offset of the `03` label tag.
    pub source_offset: u64,
}

/// Exactly bounded feature-history operation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based record order within the feature-history section.
    pub ordinal: u32,
    /// Exact record byte length.
    pub byte_len: u64,
    /// SHA-256 of the complete operation record.
    pub sha256: String,
    /// Exact serialized post-label payload length.
    pub payload_byte_len: u64,
    /// SHA-256 of the post-label serialized operation payload.
    pub payload_sha256: String,
    /// Absolute file offset of the first post-label payload byte.
    pub payload_source_offset: u64,
    /// Absolute file offset of the fixed operation-header marker.
    pub source_offset: u64,
}

/// Ordered length-framed string from one bounded feature-operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePayloadString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning exact feature-operation record.
    pub operation_record: String,
    /// Zero-based string order within the post-label payload.
    pub ordinal: u32,
    /// Exact UTF-8 string value.
    pub value: String,
    /// Absolute file offset of the `04` marker.
    pub source_offset: u64,
}

/// Typed operation template carried by a `SIMPLE HOLE` payload string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleTemplate {
    /// Globally unique template identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Source string in the native payload-string arena.
    pub payload_string: String,
    /// Hole construction family token.
    pub family: SimpleHoleFamily,
    /// Hole cross-section token.
    pub form: SimpleHoleForm,
    /// Axial extent token.
    pub extent: SimpleHoleExtent,
    /// Entry treatment token.
    pub start_treatment: SimpleHoleEndTreatment,
    /// Exit treatment token.
    pub end_treatment: SimpleHoleEndTreatment,
}

/// Exact nonempty redundantly witnessed scalar lane in a simple-hole payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleRepeatedScalarLane {
    /// Globally unique repeated-lane identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Ordered finite shifted-binary64 values.
    pub values: Vec<f64>,
    /// Absolute offsets of the first scalar lane.
    pub first_witness_offsets: Vec<u64>,
    /// Absolute offsets of the byte-identical repeated scalar lane.
    pub second_witness_offsets: Vec<u64>,
}

/// Offset-store blocks linked after both repeated scalar-lane witnesses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
    /// Globally unique reference-lane identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Ordered blocks following the first scalar pair.
    pub first_data_blocks: [String; 2],
    /// Ordered blocks following the repeated scalar lane.
    pub second_data_blocks: [String; 2],
    /// Absolute offsets of the first pair of tagged-index tokens.
    pub first_reference_offsets: [u64; 2],
    /// Absolute offsets of the repeated pair of tagged-index tokens.
    pub second_reference_offsets: [u64; 2],
}

/// Distinct simple-hole operations sharing one four-block construction identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleConstructionGroup {
    /// Globally unique group identity.
    pub id: String,
    /// Shared first-witness block pair.
    pub first_data_blocks: [String; 2],
    /// Shared repeated-witness block pair.
    pub second_data_blocks: [String; 2],
    /// Operation labels in feature-history order.
    pub operation_labels: Vec<String>,
    /// Scalar lanes aligned with `operation_labels`.
    pub scalar_lanes: Vec<String>,
    /// Block-reference lanes aligned with `operation_labels`.
    pub block_references: Vec<String>,
}

/// Construction family named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleFamily {
    /// General-hole construction family.
    GeneralHole,
}

/// Cross-section named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleForm {
    /// Plain cylindrical cross-section.
    Simple,
}

/// Axial termination named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleExtent {
    /// Continue through all intersected material.
    Through,
}

/// End treatment named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleEndTreatment {
    /// Chamfer the circular end edge.
    Chamfer,
}

/// Primary body object read or written by one feature-history operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBodyReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Primary body object index.
    pub body_object_index: u32,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Ordered body-reference field retained from one feature-history operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBodyReferenceOccurrence {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based field order within the bounded operation record.
    pub ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Unambiguous reuse of one segment body image by a primary feature body field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBodySegmentUse {
    /// Globally unique use identity.
    pub id: String,
    /// Primary field in the native `feature_body_references` arena.
    pub feature_body_reference: String,
    /// Segment image in the native `segment_body_bindings` arena.
    pub segment_body_binding: String,
}

/// Operation-header input resolved to one bounded offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInputBlock {
    /// Globally unique input-binding identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: u8,
    /// Object index serialized in that slot.
    pub object_index: u32,
    /// Target in the native `data_blocks` arena.
    pub data_block: String,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Input-block bindings from distinct operations that resolve to one data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInputBlockIdentityGroup {
    /// Globally unique group identity.
    pub id: String,
    /// Shared target in the native `data_blocks` arena.
    pub data_block: String,
    /// Input bindings in ascending source-offset order.
    pub input_blocks: Vec<String>,
    /// Operation labels aligned with `input_blocks`.
    pub operation_labels: Vec<String>,
    /// Header slots aligned with `input_blocks`.
    pub input_slots: Vec<u8>,
    /// Object-index token offsets aligned with `input_blocks`.
    pub source_offsets: Vec<u64>,
}

/// Serialized column-row grammar carrying a reused feature input block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnIndexRowKind {
    /// `2d 02 0b ... 93 8a` index row.
    Index,
    /// `02 0b ... 93 8c` linked-index row.
    LinkedIndex,
    /// `02 01 01 01 16` target-index row.
    TargetIndex,
}

impl ColumnIndexRowKind {
    const fn id_component(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::LinkedIndex => "linked-index",
            Self::TargetIndex => "target-index",
        }
    }
}

/// Exact reuse of one feature input block by any column-row slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInputColumnRowUse {
    /// Globally unique use identity.
    pub id: String,
    /// Feature input binding that resolves to the shared block.
    pub input_block: String,
    /// Owning feature operation label.
    pub operation_label: String,
    /// Input slot in the operation header.
    pub input_slot: u8,
    /// Serialized grammar of the referenced column row.
    pub row_kind: ColumnIndexRowKind,
    /// Native row identity in its grammar-specific arena.
    pub column_row: String,
    /// Unique complete composite table containing the row, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_table: Option<String>,
    /// Zero-based slot in the row's four-block lane.
    pub row_slot: u8,
    /// Exact shared target in the native `data_blocks` arena.
    pub data_block: String,
    /// Absolute file offset of the row's compact block index.
    pub source_offset: u64,
}

/// Unique composite-table target row for one feature input block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInputColumnTarget {
    /// Globally unique target identity.
    pub id: String,
    /// Feature input binding that resolves to the target block.
    pub input_block: String,
    /// Owning feature operation label.
    pub operation_label: String,
    /// Input slot in the operation header.
    pub input_slot: u8,
    /// Linked or target-index row whose slot zero is the input block.
    pub column_row: String,
    /// Serialized grammar of `column_row`.
    pub row_kind: ColumnIndexRowKind,
    /// Leading compact value present only in the linked-row grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_index: Option<u32>,
    /// Absolute offset of `leading_index` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_index_source_offset: Option<u64>,
    /// Linked-row discriminator when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discriminator: Option<u8>,
    /// Three compact values following the fixed row marker.
    pub field_indices: [u32; 3],
    /// Three same-section blocks addressed by `field_indices`.
    pub field_data_blocks: [String; 3],
    /// Absolute offsets of the three compact field values.
    pub field_source_offsets: [u64; 3],
    /// Linked-row flag when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<u8>,
    /// Serialized row mode.
    pub mode: u8,
    /// Unique complete composite table containing `column_row`.
    pub column_table: String,
    /// Exact target in the native `data_blocks` arena.
    pub data_block: String,
    /// Absolute file offset of the row's target block index.
    pub source_offset: u64,
}

/// Ordered parameter declaration reached through one feature input block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureParameterBinding {
    /// Globally unique binding identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: u8,
    /// Input block carrying the object-reference field.
    pub input_block: String,
    /// Zero-based object-reference order within the input block.
    pub reference_ordinal: u32,
    /// Target parameter declaration in the native expression arena.
    pub expression_declaration: String,
    /// Exact numeric expression bound to the declaration, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// Persistent OM object ID of the declaration.
    pub object_id: u32,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// All binding occurrences by which one operation consumes one expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureParameterUse {
    /// Globally unique use identity.
    pub id: String,
    /// Consuming operation-label identity.
    pub operation_label: String,
    /// Exact numeric expression consumed by the operation.
    pub expression: String,
    /// Binding occurrences in ascending source-offset order.
    pub bindings: Vec<String>,
    /// Binding source offsets aligned with `bindings`.
    pub source_offsets: Vec<u64>,
}

/// Ordered sketch-history record and its exact native input lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchRecord {
    /// Globally unique sketch-record identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Zero-based order within the feature-history area.
    pub ordinal: u32,
    /// Exact bounded operation record.
    pub operation_record: String,
    /// Resolved input bindings in header-slot order.
    pub input_blocks: Vec<String>,
    /// Ordered references carried by the sketch payload.
    pub payload_references: Vec<String>,
    /// Absolute file offset of the operation label.
    pub source_offset: u64,
}

/// Completely resolved native construction lane of a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumCsysConstruction {
    /// Globally unique construction identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Payload control byte preceding the fixed construction header suffix.
    pub control: u8,
    /// Eight object indices in serialized lane order.
    pub object_indices: [u32; 8],
    /// Eight uniquely resolved same-store blocks in lane order.
    pub data_blocks: [String; 8],
    /// Absolute offsets of the eight canonical reference markers.
    pub source_offsets: [u64; 8],
}

/// Exact logical payload reconstructed from the two leading datum-CSYS blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumCsysPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Construction defining the ordered block lane.
    pub construction: String,
    /// Two leading source blocks in serialized order.
    pub data_blocks: [String; 2],
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Payload-relative block starts.
    pub block_payload_offsets: [u64; 2],
    /// Exact source-block lengths.
    pub block_byte_lengths: [u64; 2],
    /// Absolute source-block offsets.
    pub block_source_offsets: [u64; 2],
}

/// One exactly framed scalar pair in a reconstructed datum-CSYS payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDatumCsysPayloadScalarPair {
    /// Globally unique scalar-pair identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    pub datum_csys_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered finite shifted-IEEE values.
    pub values: [f64; 2],
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative scalar offsets.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    pub value_source_offsets: [u64; 2],
    /// Exact discriminator selecting the scalar-pair branch.
    pub discriminator: Vec<u8>,
}

/// One exactly framed signed Q1.55 pair in a reconstructed datum-CSYS payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDatumCsysPayloadFixedPair {
    /// Globally unique fixed-pair identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    pub datum_csys_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered dimensionless Q1.55 values.
    pub values: [f64; 2],
    /// Exact seven-byte two's-complement payloads.
    pub raw_values: [[u8; 7]; 2],
    /// Exact discriminator selecting the pair branch.
    pub discriminator: Vec<u8>,
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the two `30` atom markers.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the two `30` atom markers.
    pub value_source_offsets: [u64; 2],
}

/// One exactly framed scalar field in a reconstructed datum-CSYS payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDatumCsysPayloadScalar {
    /// Globally unique scalar-field identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the field.
    pub datum_csys_payload: String,
    /// Zero-based field order within the payload.
    pub ordinal: u32,
    /// Serialized discriminator following the `50 59 66` marker.
    pub field_code: u8,
    /// Finite shifted-IEEE binary64 value.
    pub value: f64,
    /// Payload-relative offset of the field marker.
    pub payload_offset: u64,
    /// Absolute source offset of the field marker.
    pub source_offset: u64,
}

/// Typed descriptor from one of the final three datum-CSYS construction lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumCsysDescriptor {
    /// Globally unique descriptor identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Construction carrying the descriptor lane.
    pub construction: String,
    /// Construction reference ordinal in the range 5–7.
    pub reference_ordinal: u8,
    /// Resolved source block.
    pub data_block: String,
    /// Exact bytes preceding the hexadecimal identity.
    pub prefix: Vec<u8>,
    /// Lowercase 30–32 digit hexadecimal identity.
    pub identity: String,
    /// Exact bytes following the hexadecimal identity.
    pub suffix: Vec<u8>,
    /// Absolute source offset of the block.
    pub source_offset: u64,
    /// Absolute source offset of the identity.
    pub identity_source_offset: u64,
}

/// Exact shared descriptor identity between datum-plane and datum-CSYS history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlaneCsysIdentityUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Shared lowercase hexadecimal identity.
    pub identity: String,
    /// Typed datum-plane descriptor.
    pub datum_plane_descriptor: String,
    /// Datum-plane operation carrying the descriptor.
    pub datum_plane_operation_label: String,
    /// Typed datum-CSYS descriptor.
    pub datum_csys_descriptor: String,
    /// Datum-CSYS operation carrying the descriptor.
    pub datum_csys_operation_label: String,
    /// Datum-CSYS construction reference ordinal.
    pub datum_csys_reference_ordinal: u8,
}

/// Common typed header of one datum-plane construction payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlaneHeader {
    /// Globally unique header identity.
    pub id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub operation_label: String,
    /// Payload control byte.
    pub control: u8,
    /// Declared construction count.
    pub declared_count: u8,
    /// Tag selecting the following construction branch.
    pub branch_tag: u8,
    /// Ordered compact descriptor indices carried by the selected branch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descriptor_indices: Vec<u32>,
    /// Ordered canonical object indices carried by the selected branch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_indices: Vec<u32>,
    /// Atomically resolved same-store descriptor blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descriptor_data_blocks: Vec<String>,
    /// Atomically resolved same-store canonical object blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_data_blocks: Vec<String>,
    /// Absolute offsets of compact descriptor indices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descriptor_source_offsets: Vec<u64>,
    /// Absolute offsets of canonical object-index markers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_source_offsets: Vec<u64>,
    /// Absolute offset of the payload control byte.
    pub source_offset: u64,
}

/// Exact logical datum-plane object payload reconstructed in lane order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlanePayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub operation_label: String,
    /// Header defining the ordered object-block lane.
    pub datum_plane_header: String,
    /// Ordered source blocks.
    pub data_blocks: Vec<String>,
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Starting payload offset of each source block.
    pub block_payload_offsets: Vec<u64>,
    /// Exact byte length of each source block.
    pub block_byte_lengths: Vec<u64>,
    /// Absolute file offset of each source block.
    pub block_source_offsets: Vec<u64>,
    /// Payload-relative opening offset of a unique terminal index lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_lane_offset: Option<u64>,
    /// Declared count of the terminal index lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_lane_declared_count: Option<u8>,
    /// Ordered decoded compact indices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_lane_values: Vec<u32>,
    /// Payload-relative offsets of decoded compact indices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_lane_value_offsets: Vec<u64>,
    /// Big-endian trailer word of the terminal lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_lane_trailer: Option<u32>,
}

/// One exactly framed scalar pair in a reconstructed datum-plane payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDatumPlanePayloadScalarPair {
    /// Globally unique scalar-pair identity.
    pub id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    pub datum_plane_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered finite shifted-IEEE values.
    pub values: [f64; 2],
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the scalar encodings.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    pub value_source_offsets: [u64; 2],
}

/// Resolved typed descriptor of one datum-plane construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlaneDescriptor {
    /// Globally unique descriptor identity.
    pub id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub operation_label: String,
    /// Header carrying the descriptor reference.
    pub datum_plane_header: String,
    /// Zero-based descriptor-lane order.
    pub ordinal: u32,
    /// Resolved source block.
    pub data_block: String,
    /// Lowercase hexadecimal identity preceding the delimiter.
    pub identity: String,
    /// Exact descriptor suffix beginning with `?`.
    pub suffix: Vec<u8>,
    /// Non-null compact schema index following `?A`.
    pub schema_index: u32,
    /// Nonempty printable terminal label.
    pub label: String,
    /// Absolute source offset of the descriptor block.
    pub source_offset: u64,
}

/// Datum-plane construction lane containing a reused block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatumPlaneBlockLane {
    /// Compact descriptor lane.
    Descriptor,
    /// Canonical object-reference lane.
    Object,
}

/// Exact reuse of one resolved datum-plane construction block by an operation input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlaneBlockUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Owning datum-plane header.
    pub datum_plane_header: String,
    /// `DATUM_PLANE` operation owning the construction.
    pub construction_operation_label: String,
    /// Construction lane containing the block.
    pub lane: DatumPlaneBlockLane,
    /// Zero-based position within the lane.
    pub reference_ordinal: u32,
    /// Shared offset-store block.
    pub data_block: String,
    /// Matching operation-header input binding.
    pub input_binding: String,
    /// Operation whose header addresses the shared block.
    pub input_operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: u8,
}

/// Exact reuse of one datum-coordinate-system construction block by an operation input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumCsysBlockUse {
    /// Globally unique block-use identity.
    pub id: String,
    /// Owning datum-coordinate-system construction.
    pub construction: String,
    /// `DATUM_CSYS` operation owning the construction lane.
    pub construction_operation_label: String,
    /// Zero-based position in the eight-reference construction lane.
    pub reference_ordinal: u8,
    /// Shared offset-store block.
    pub data_block: String,
    /// Matching operation-header input binding.
    pub input_binding: String,
    /// Operation whose header addresses the shared block.
    pub input_operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: u8,
}

/// Completely resolved counted-reference field of one sketch construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchConstructionInputs {
    /// Globally unique construction-input identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Joined typed sketch record.
    pub sketch_record: String,
    /// Ordered references preceding the field separator.
    pub member_references: Vec<String>,
    /// Ordered uniquely resolved member blocks.
    pub member_data_blocks: Vec<String>,
    /// Reference following the field separator.
    pub terminal_reference: String,
    /// Uniquely resolved terminal block.
    pub terminal_data_block: String,
}

/// Exact logical sketch payload reconstructed from its ordered store blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Complete construction-input record defining block order.
    pub construction_inputs: String,
    /// Ordered leading member blocks followed by the separated terminal block.
    pub data_blocks: Vec<String>,
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the exact concatenated payload bytes.
    pub sha256: String,
    /// Starting payload offset of each block in concatenation order.
    pub block_payload_offsets: Vec<u64>,
    /// Exact serialized length of each source block.
    pub block_byte_lengths: Vec<u64>,
    /// Absolute file offset of each source block.
    pub block_source_offsets: Vec<u64>,
}

/// One exactly framed coordinate pair in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPayloadCoordinatePair {
    /// Globally unique coordinate-pair identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    pub construction_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered finite shifted-IEEE values.
    pub values: [f64; 2],
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative scalar offsets.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    pub value_source_offsets: [u64; 2],
    /// Exact discriminator selecting the coordinate-pair branch.
    pub discriminator: Vec<u8>,
}

/// One exactly framed signed fixed-point pair in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPayloadFixedPair {
    /// Globally unique fixed-pair identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    pub construction_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered dimensionless signed Q1.55 values.
    pub values: [f64; 2],
    /// Exact ordered seven-byte two's-complement payloads.
    pub raw_values: [[u8; 7]; 2],
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the two atom markers.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the two atom markers.
    pub value_source_offsets: [u64; 2],
}

/// Exact framed scalar retained from one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPayloadScalar {
    /// Globally unique scalar identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying this field.
    pub construction_payload: String,
    /// Zero-based field order within the reconstructed payload.
    pub ordinal: u32,
    /// Serialized discriminator following the `50 59 66` marker.
    pub field_code: u8,
    /// Finite shifted-IEEE binary64 value.
    pub value: f64,
    /// Byte offset of the field marker within the reconstructed payload.
    pub payload_offset: u64,
    /// Absolute file offset of the field marker.
    pub source_offset: u64,
}

/// Exact framed name retained from one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchPayloadName {
    /// Globally unique name-field identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying this field.
    pub construction_payload: String,
    /// Zero-based name-field order within the reconstructed payload.
    pub ordinal: u32,
    /// Decoded compact type code following the `66` marker, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_code: Option<u32>,
    /// Whether the field uses the type-free payload-leading form.
    pub payload_leading: bool,
    /// Exact printable field value.
    pub value: String,
    /// Byte offset of the `66` marker within the reconstructed payload.
    pub payload_offset: u64,
    /// Absolute file offset of the `66` marker.
    pub source_offset: u64,
}

/// Named sketch payload interval and its ordered framed numeric fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchPayloadNamedRecord {
    /// Globally unique named-record identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying this record.
    pub construction_payload: String,
    /// Name field opening the retained interval.
    pub name_field: String,
    /// Ordered scalar fields before the next complete name field.
    pub scalar_fields: Vec<String>,
    /// Ordered fixed-pair fields before the next complete name field.
    pub fixed_pairs: Vec<String>,
    /// Payload-relative offset of the opening name marker.
    pub payload_start_offset: u64,
    /// Payload-relative exclusive end at the next name or payload boundary.
    pub payload_end_offset: u64,
}

/// Complete named two-dimensional point in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPoint {
    /// Globally unique point identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Name-delimited payload record carrying the point.
    pub named_record: String,
    /// Exact `Point<decimal>` source name.
    pub name: String,
    /// Ordered scalar fields carrying the two coordinates.
    pub scalar_fields: [String; 2],
    /// Ordered finite native coordinate values.
    pub coordinates: [f64; 2],
}

/// Complete named dimensionless fixed-point record in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchFixedPoint {
    /// Globally unique fixed-point identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Name-delimited payload record carrying the pair.
    pub named_record: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Exact fixed-pair field carrying the two values.
    pub fixed_pair: String,
    /// Ordered dimensionless signed Q1.55 values.
    pub values: [f64; 2],
    /// Absolute source offset of the fixed-pair discriminator.
    pub source_offset: u64,
}

/// Exact same-name point identity within one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPointGroup {
    /// Globally unique point-group identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Identical point records in payload order.
    pub points: Vec<String>,
    /// Bit-identical ordered coordinate values.
    pub coordinates: [f64; 2],
}

/// Named two-scalar point object spanning consecutive offset-store blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffsetStoreNamedPoint {
    /// Globally unique point-object identity.
    pub id: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Minimal consecutive source-block span carrying the object.
    pub data_blocks: Vec<String>,
    /// Ordered finite native scalar values.
    pub values: [f64; 2],
    /// Absolute source offsets of the two scalar markers.
    pub value_source_offsets: [u64; 2],
    /// Absolute source offset of the name frame.
    pub source_offset: u64,
}

/// Exact reuse of one named-point block by a sketch reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchNamedPointBlockUse {
    /// Globally unique block-use identity.
    pub id: String,
    /// Sketch operation carrying the reference.
    pub operation_label: String,
    /// Typed sketch-reference occurrence.
    pub sketch_reference: String,
    /// Reference order within the sketch field.
    pub reference_ordinal: u32,
    /// Typed named-point object containing the block.
    pub named_point: String,
    /// Shared offset-store block.
    pub data_block: String,
    /// Block position within the named-point span.
    pub point_block_ordinal: u32,
    /// Absolute source offset of the sketch reference.
    pub source_offset: u64,
}

/// Exact predecessor relation between a named point and a sketch construction lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchPrecedingNamedPointUse {
    /// Globally unique predecessor-use identity.
    pub id: String,
    /// Sketch operation carrying the construction lane.
    pub operation_label: String,
    /// First typed sketch-reference occurrence.
    pub first_sketch_reference: String,
    /// Typed named-point object ending immediately before the construction lane.
    pub named_point: String,
    /// Complete ordered block span of the named point.
    pub point_data_blocks: Vec<String>,
    /// First construction block immediately following the point span.
    pub following_data_block: String,
    /// Absolute source offset of the first sketch reference.
    pub source_offset: u64,
}

/// Exact identity of one solved sketch point across its payload and reference lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchPointUse {
    /// Globally unique point-use identity.
    pub id: String,
    /// Sketch operation carrying both point encodings.
    pub operation_label: String,
    /// Ordered sketch-reference occurrences addressing the named-point span.
    pub sketch_references: Vec<String>,
    /// Exact block-use witnesses corresponding to the sketch references.
    pub block_uses: Vec<String>,
    /// Exact same-name sketch-point group.
    pub sketch_point_group: String,
    /// Independently framed named-point object addressed by the reference.
    pub named_point: String,
    /// Absolute source offsets of the sketch references.
    pub source_offsets: Vec<u64>,
}

/// Exact ordered dependency from a sketch point to a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FeatureSketchDatumCsysBlockRelation {
    /// The named-point span and coordinate-system construction address one block.
    Shared {
        /// Block addressed by both the named-point span and construction.
        data_block: String,
    },
    /// The coordinate-system construction begins immediately after the named-point span.
    Consecutive {
        /// Final block in the complete named-point span.
        point_data_block: String,
        /// First block in the coordinate-system construction.
        construction_data_block: String,
    },
}

/// Exact ordered dependency from a sketch point to a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchDatumCsysDependency {
    /// Globally unique dependency identity.
    pub id: String,
    /// Earlier sketch operation owning the point identity.
    pub sketch_operation_label: String,
    /// Later datum-coordinate-system operation consuming the point block.
    pub datum_csys_operation_label: String,
    /// Exact sketch-point identity witnessing ownership.
    pub sketch_point_use: String,
    /// Exact datum-coordinate-system construction witnessing consumption.
    pub datum_csys_construction: String,
    /// Exact block relation between the complete point span and construction.
    pub block_relation: FeatureSketchDatumCsysBlockRelation,
    /// Absolute source offset of the first sketch reference witnessing the point identity.
    pub source_offset: u64,
}

/// Ordered object reference carried by a bounded sketch-operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchReference {
    /// Globally unique sketch-reference identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Zero-based reference order in the counted field.
    pub ordinal: u32,
    /// Effective count encoded by the containing reference field.
    pub declared_count: u8,
    /// Whether this is the reference following the `00 00` separator.
    pub terminal: bool,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Ordered construction reference carried by a bounded projected-curve payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureProjectedCurveReference {
    /// Globally unique projected-curve reference identity.
    pub id: String,
    /// Owning `CPROJ` or `CPROJ_CMB` operation label.
    pub operation_label: String,
    /// Zero-based order among the field's non-repeated references.
    pub ordinal: u32,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Ordered construction reference carried by a bounded pattern payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePatternReference {
    /// Globally unique pattern-reference identity.
    pub id: String,
    /// Owning pattern operation label.
    pub operation_label: String,
    /// Zero-based non-null slot order in the exact reference field.
    pub ordinal: u32,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Exact leading construction header carried by a bounded point-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePointConstructionHeader {
    /// Globally unique point-construction-header identity.
    pub id: String,
    /// Owning `POINT` operation label.
    pub operation_label: String,
    /// Serialized construction object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Serialized header mode.
    pub mode: u8,
    /// Absolute file offset of the reference width marker.
    pub source_offset: u64,
}

/// Exact cross-block scalar lane selected by a point-construction header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaturePointConstructionScalarLane {
    /// Globally unique point-construction scalar-lane identity.
    pub id: String,
    /// Owning `POINT` operation label.
    pub operation_label: String,
    /// Header selecting this lane.
    pub construction_header: String,
    /// Preceding and target data blocks in byte order.
    pub data_blocks: [String; 2],
    /// Six finite scalar values in byte order.
    pub values: [f64; 6],
    /// Absolute file offsets of the six scalar markers.
    pub source_offsets: [u64; 6],
}

/// Ordered construction reference carried by a bounded draft-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionReference {
    /// Globally unique draft-construction-reference identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Zero-based slot order in the exact construction graph.
    pub ordinal: u32,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Counted compact-index lane preceding a bounded draft construction graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionIndexLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Serialized count including the omitted lane owner.
    pub declared_count: u8,
    /// Non-null compact indices in serialized order.
    pub indices: Vec<u32>,
    /// Same-store native blocks when the complete lane and graph select one store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_blocks: Option<Vec<String>>,
    /// Absolute source offsets of the compact-index tokens.
    pub source_offsets: Vec<u64>,
}

/// Exact logical payload reconstructed from a resolved draft index lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Counted index lane defining the ordered block sequence.
    pub index_lane: String,
    /// Ordered source blocks.
    pub data_blocks: Vec<String>,
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Payload-relative block starts.
    pub block_payload_offsets: Vec<u64>,
    /// Exact source-block lengths.
    pub block_byte_lengths: Vec<u64>,
    /// Absolute source-block offsets.
    pub block_source_offsets: Vec<u64>,
}

/// End-anchored compact-index lane in a bounded draft construction payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionTerminalLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Two non-null compact indices in serialized order.
    pub indices: [u32; 2],
    /// Exact uninterpreted bytes preceding the terminal zero.
    pub tail: [u8; 3],
    /// Absolute source offsets of the compact-index tokens.
    pub index_source_offsets: [u64; 2],
    /// Absolute source offset of the first compact-index token.
    pub source_offset: u64,
}

/// Ordered construction reference carried by a bounded surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceConstructionReference {
    /// Globally unique surface-construction-reference identity.
    pub id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub operation_label: String,
    /// Zero-based slot order in the exact common envelope.
    pub ordinal: u32,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// One resolved reference within a surface-construction branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceBranchReference {
    /// Zero-based member order, or the declared count minus one for the terminal.
    pub ordinal: u32,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// One exact counted branch in a bounded surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceConstructionBranch {
    /// Globally unique branch identity.
    pub id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub operation_label: String,
    /// Zero-based branch order.
    pub ordinal: u32,
    /// Serialized construction family byte following `a0 5a`.
    pub family: u8,
    /// Serialized branch-group header code.
    pub header_code: u8,
    /// Serialized `16` or `40` branch mode.
    pub mode: u8,
    /// Count including the terminal reference.
    pub declared_count: u8,
    /// Whether the payload repeats the declared count before its zero lane.
    pub witnessed: bool,
    /// Ordered nonterminal references.
    pub members: Vec<FeatureSurfaceBranchReference>,
    /// Terminal reference.
    pub terminal: FeatureSurfaceBranchReference,
    /// Opaque bytes separating the terminal from the next branch or terminator.
    pub suffix: Vec<u8>,
    /// Absolute file offset of the branch mode byte.
    pub source_offset: u64,
}

/// Ordered profile reference carried by a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureExtrudeProfileReference {
    /// Globally unique profile-reference identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Zero-based profile-reference order.
    pub ordinal: u32,
    /// Whether the payload repeats the complete encoded profile list exactly once.
    pub witnessed: bool,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Fixed shifted-IEEE scalar header from a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureExtrudePayloadHeader {
    /// Globally unique header identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Ordered finite scalar values.
    pub scalars: [f64; 2],
    /// Absolute file offset of the first shifted-IEEE scalar.
    pub source_offset: u64,
}

/// Exact terminal discriminator lane from a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureExtrudePayloadFooter {
    /// Globally unique footer identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Two compact type indices following the footer prelude.
    pub type_indices: [u32; 2],
    /// Two values in the counted footer lane.
    pub mode_indices: [u32; 2],
    /// Four serialized one-byte flags.
    pub flags: [u8; 4],
    /// Compact values preceding the payload terminator.
    pub trailing_indices: Vec<u32>,
    /// Absolute file offset of the footer prelude.
    pub source_offset: u64,
}

/// Serialized width form of an extrusion payload scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeaturePayloadScalarEncoding {
    /// Single-byte exact zero.
    Zero,
    /// Four-byte shifted IEEE-754 binary32.
    Binary32,
    /// Eight-byte shifted IEEE-754 binary64.
    Binary64,
}

/// Three typed scalars anchored to an ordered operation body reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureOperationBodyScalarTriple {
    /// Globally unique scalar-clause identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: u8,
    /// Ordered finite scalar values.
    pub values: [f64; 3],
    /// Ordered serialized width forms.
    pub encodings: [FeaturePayloadScalarEncoding; 3],
    /// Absolute file offsets of the three scalar markers.
    pub source_offsets: [u64; 3],
}

/// Ordered member index in a branch-`11` operation body clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBodyMember {
    /// Globally unique member identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Zero-based member order in the counted lane.
    pub ordinal: u32,
    /// Decoded compact index.
    pub member_index: u32,
    /// Absolute file offset of the compact-index marker.
    pub source_offset: u64,
}

/// Wrapped operation member resolved in the feature-body identity namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBodyOperand {
    /// Globally unique operand identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Body clause containing the operand.
    pub body_object_index: u32,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Zero-based operand order in the wrapped member lane.
    pub ordinal: u32,
    /// Serialized operand body object index.
    pub operand_object_index: u32,
    /// Segment body bindings naming the same body image.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segment_body_bindings: Vec<String>,
    /// Absolute file offset of the compact-index marker.
    pub source_offset: u64,
}

impl FeatureOperationBodyOperand {
    pub(crate) fn source_property_key(&self) -> String {
        format!(
            "operation_body_operand.{}.{}",
            self.body_reference_ordinal, self.ordinal
        )
    }
}

/// Exact continuation following a `TRIM BODY` branch-`11` member lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBody11Continuation {
    /// Globally unique continuation identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Compact index in the single-entry continuation lane.
    pub continuation_index: u32,
    /// Absolute file offset of the continuation compact-index marker.
    pub continuation_source_offset: u64,
    /// Object index in the terminal field.
    pub terminal_object_index: u32,
    /// Absolute file offset of the terminal object-index marker.
    pub terminal_source_offset: u64,
}

/// Homogeneous value encoding in an operation body-reference lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureOperationBodyReferenceLaneEncoding {
    /// NX OM compact-index encoding.
    CompactIndex,
    /// `f0`/`f1` payload object-index encoding.
    PayloadObjectIndex,
}

/// Counted reference lane following an operation body scalar clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBodyReferenceLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: u8,
    /// Homogeneous encoding used by every lane value.
    pub encoding: FeatureOperationBodyReferenceLaneEncoding,
    /// Ordered decoded indices.
    pub object_indices: Vec<u32>,
    /// Unique offset-only data blocks addressed by the ordered indices.
    pub data_blocks: Vec<Option<String>>,
    /// Absolute file offsets of the encoded index markers.
    pub source_offsets: Vec<u64>,
}

/// Atomically witnessed extrusion construction profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureExtrudeConstructionProfile {
    /// Globally unique construction-profile identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Body object anchoring the branch-`11` construction clause.
    pub body_object_index: u32,
    /// Ordered serialized profile object indices.
    pub object_indices: Vec<u32>,
    /// Ordered uniquely resolved profile data blocks.
    pub data_blocks: Vec<String>,
    /// Source offsets from the independently encoded profile field.
    pub profile_source_offsets: Vec<u64>,
    /// Source offsets from the body-clause reference lane.
    pub body_lane_source_offsets: Vec<u64>,
}

/// Structured `32` branch following an extrusion body-reference field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureExtrudePayload32Branch {
    /// Globally unique branch identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Body object index anchoring the branch.
    pub body_object_index: u32,
    /// Finite shifted-IEEE scalar following the branch marker.
    pub scalar: f64,
    /// Ordered fixed-width big-endian atoms in the first counted lane.
    pub atoms_be: Vec<u32>,
    /// Compact indices wrapped by the fixed-width atoms.
    pub atom_indices: Vec<u32>,
    /// Unique offset-only data blocks addressed by the atom indices.
    pub atom_data_blocks: Vec<Option<String>>,
    /// Ordered values in the first compact-index lane.
    pub first_indices: Vec<u32>,
    /// Unique offset-only data blocks addressed by the first lane.
    pub first_data_blocks: Vec<Option<String>>,
    /// Ordered values in the second compact-index lane.
    pub second_indices: Vec<u32>,
    /// Unique offset-only data blocks addressed by the second lane.
    pub second_data_blocks: Vec<Option<String>>,
    /// Object index in the terminal field.
    pub terminal_object_index: u32,
    /// Absolute file offset of the `32` branch marker.
    pub source_offset: u64,
}

/// Complete alternate extrusion construction using the structured `32` branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureExtrude32Construction {
    /// Globally unique construction identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Structured branch supplying the body-anchored construction lanes.
    pub branch: String,
    /// Body object index witnessed at both ends of the structured branch.
    pub body_object_index: u32,
    /// Ordered profile-reference identities.
    pub profile_references: Vec<String>,
    /// Ordered uniquely resolved profile blocks.
    pub profile_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the fixed-atom lane.
    pub atom_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the first compact-index lane.
    pub first_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the second compact-index lane.
    pub second_data_blocks: Vec<String>,
}

/// Ordered construction reference carried by a bounded `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockConstructionReference {
    /// Globally unique construction-reference identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Payload control byte preceding the construction field.
    pub control: u8,
    /// Zero-based reference order across the complete field.
    pub ordinal: u32,
    /// Whether this is the reference following the separator byte.
    pub terminal: bool,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Completely resolved construction-reference field of one `BLOCK` feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockConstruction {
    /// Globally unique construction identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Payload control byte preceding the construction field.
    pub control: u8,
    /// Eighteen ordered references preceding the separator.
    pub member_references: Vec<String>,
    /// Eighteen uniquely resolved member blocks.
    pub member_data_blocks: Vec<String>,
    /// Reference following the separator.
    pub terminal_reference: String,
    /// Uniquely resolved terminal block.
    pub terminal_data_block: String,
}

/// Exact logical payload reconstructed from one complete `BLOCK` construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Construction defining serialized block order.
    pub construction: String,
    /// Ordered member blocks followed by the terminal block.
    pub data_blocks: Vec<String>,
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Payload-relative source-block starts.
    pub block_payload_offsets: Vec<u64>,
    /// Exact source-block lengths.
    pub block_byte_lengths: Vec<u64>,
    /// Absolute source-block offsets.
    pub block_source_offsets: Vec<u64>,
}

/// One complete shifted-binary64 field in a reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadScalar {
    /// Globally unique scalar-field identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Reconstructed payload containing the field.
    pub construction_payload: String,
    /// Zero-based field order in the reconstructed payload.
    pub ordinal: u32,
    /// Serialized field discriminator following `PYf`.
    pub field_code: u8,
    /// Exact finite shifted-binary64 value.
    pub value: f64,
    /// Payload-relative `PYf` marker offset.
    pub payload_offset: u64,
    /// Absolute source offset of the `PYf` marker.
    pub source_offset: u64,
}

/// One complete compact-code name field in a reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadName {
    /// Globally unique name-field identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Reconstructed payload containing the field.
    pub construction_payload: String,
    /// Zero-based name order in the reconstructed payload.
    pub ordinal: u32,
    /// Non-null compact type code, absent for the payload-leading form.
    pub type_code: Option<u32>,
    /// Whether the field uses the type-free payload-leading form.
    pub payload_leading: bool,
    /// Exact printable field value.
    pub value: String,
    /// Payload-relative name marker offset.
    pub payload_offset: u64,
    /// Absolute source offset of the name marker.
    pub source_offset: u64,
}

/// Name-delimited interval in a reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadNamedRecord {
    /// Globally unique interval identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Reconstructed payload containing the interval.
    pub construction_payload: String,
    /// Name field opening the interval.
    pub name_field: String,
    /// Complete scalar fields in payload order within the interval.
    pub scalar_fields: Vec<String>,
    /// Inclusive payload-relative start.
    pub payload_start_offset: u64,
    /// Exclusive payload-relative end.
    pub payload_end_offset: u64,
}

/// Exactly two-scalar `Point<positive decimal>` record in a `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadPoint {
    /// Globally unique typed-point identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Name-delimited payload interval carrying the point.
    pub named_record: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Ordered scalar fields carrying the two coordinates.
    pub scalar_fields: [String; 2],
    /// Ordered finite native coordinate values.
    pub coordinates: [f64; 2],
}

/// Exact same-name point identity within one reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadPointGroup {
    /// Globally unique point-group identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Identical point records in payload order.
    pub points: Vec<String>,
    /// Bit-identical ordered coordinate values.
    pub coordinates: [f64; 2],
}

/// Ordered three-parameter dimension run of one `BLOCK` feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureBlockDimensions {
    /// Globally unique dimension-set identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Complete resolved block construction.
    pub construction: String,
    /// Bindings selecting the first parameter declaration.
    pub anchor_bindings: Vec<String>,
    /// Ordered consecutive parameter declarations.
    pub declarations: [String; 3],
    /// Ordered exact numeric expression records.
    pub expressions: [String; 3],
    /// Ordered finite dimensions in model millimeters.
    pub values: [f64; 3],
}

/// Persistent object frame carried by one bounded offset-store block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockObjectFrame {
    /// Globally unique relation identity.
    pub id: String,
    /// Source block carrying the object frame.
    pub data_block: String,
    /// Zero-based frame order within the block.
    pub ordinal: u32,
    /// Serialized persistent object ID.
    pub object_id: u32,
    /// Absolute source offset of the compact object index.
    pub source_offset: u64,
}

/// Feature-history Boolean operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureBooleanKind {
    /// Add tool bodies to the target.
    Unite,
    /// Remove tool bodies from the target.
    Subtract,
    /// Retain target/tool intersections.
    Intersect,
}

/// Ordered target/tool binding from a feature-history Boolean operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBooleanOperation {
    /// Globally unique Boolean identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Boolean operation kind.
    pub kind: FeatureBooleanKind,
    /// Object index of the target body.
    pub target_object_index: u32,
    /// Ordered object indices of the tool bodies.
    pub tool_object_indices: Vec<u32>,
    /// Absolute file offset of the operation label tag.
    pub source_offset: u64,
}

/// Decode internally pointed record areas from linked OM sections.
pub fn om_record_areas(container: &Container) -> Vec<OmRecordArea> {
    let links = segment_om_links(container);
    let sections = container.om_sections();
    links
        .into_iter()
        .filter_map(|link| {
            let section = sections
                .iter()
                .find(|(entry, section)| {
                    entry
                        .file_span
                        .map_or(section.offset as u64, |(offset, _)| {
                            offset + section.offset as u64
                        })
                        == link.section_offset
                })?
                .1
                .clone();
            let header = section.record_area_header()?;
            let bytes = section.record_area?;
            let entry_offset = link.section_offset.checked_sub(section.offset as u64)?;
            let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
            Some(OmRecordArea {
                id: format!("nx:om-record-areas:area#{section_key}-{}", header.offset),
                section_link: link.id,
                schema_role: link.schema_role,
                control_words: header.control_words,
                product_version: header.product.value.to_string(),
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                source_offset: entry_offset + header.offset as u64,
            })
        })
        .collect()
}

fn feature_history_sections(container: &Container) -> Vec<(usize, SegmentOmLink)> {
    canonical_feature_history_links(segment_om_links(container))
        .into_iter()
        .enumerate()
        .collect()
}

fn visit_feature_history_operation_records(
    container: &Container,
    mut visit: impl FnMut(&str, u64, usize, crate::om::OperationRecord<'_>),
) {
    let sections = container.om_sections();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            visit(&section_key, entry_offset, operation_ordinal, record);
        }
    }
}

pub(crate) fn canonical_feature_history_links(
    links: impl IntoIterator<Item = SegmentOmLink>,
) -> Vec<SegmentOmLink> {
    let mut links = links
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
        .collect::<Vec<_>>();
    links.sort_by(|first, second| {
        first
            .section_offset
            .cmp(&second.section_offset)
            .then_with(|| first.source_offset.cmp(&second.source_offset))
            .then_with(|| first.id.cmp(&second.id))
    });
    links.dedup_by_key(|link| link.section_offset);
    links
}

/// Decode ordered operation labels from feature-history record areas.
pub fn feature_operation_labels(container: &Container) -> Vec<FeatureOperationLabel> {
    let sections = container.om_sections();
    let mut labels = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        labels.extend(section.operation_labels().into_iter().enumerate().map(
            |(ordinal, label)| FeatureOperationLabel {
                id: format!("nx:feature-history:operation-label#{section_key}-{ordinal:010}"),
                section_link: link.id.clone(),
                ordinal: ordinal as u32,
                value: label.value.to_string(),
                object_indices: label.object_indices,
                source_offset: entry_offset + label.offset as u64,
            },
        ));
    }
    labels
}

/// Decode ordered Boolean target/tool bindings from feature-history sections.
pub fn feature_boolean_operations(container: &Container) -> Vec<FeatureBooleanOperation> {
    let sections = container.om_sections();
    let mut operations = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let labels = section.operation_labels();
        for operation in section.boolean_operations() {
            let Some(ordinal) = labels
                .iter()
                .position(|label| label.offset == operation.offset)
            else {
                continue;
            };
            let kind = match operation.kind {
                crate::om::BooleanOperationKind::Unite => FeatureBooleanKind::Unite,
                crate::om::BooleanOperationKind::Subtract => FeatureBooleanKind::Subtract,
                crate::om::BooleanOperationKind::Intersect => FeatureBooleanKind::Intersect,
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{ordinal:010}");
            operations.push(FeatureBooleanOperation {
                id: format!("nx:feature-history:boolean#{section_key}-{ordinal:010}"),
                operation_label,
                kind,
                target_object_index: operation.target,
                tool_object_indices: operation.tools,
                source_offset: entry_offset + operation.offset as u64,
            });
        }
    }
    operations
}

/// Decode exact feature-operation record boundaries and byte identities.
pub fn feature_operation_records(container: &Container) -> Vec<FeatureOperationRecord> {
    let sections = container.om_sections();
    let mut records = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let labels = section.operation_labels();
        records.extend(
            section
                .operation_records()
                .into_iter()
                .filter_map(|record| {
                    let ordinal = labels
                        .iter()
                        .position(|label| label.offset == record.label.offset)?;
                    let operation_label =
                        format!("nx:feature-history:operation-label#{section_key}-{ordinal:010}");
                    Some(FeatureOperationRecord {
                        id: format!(
                            "nx:feature-history:operation-record#{section_key}-{ordinal:010}"
                        ),
                        operation_label,
                        ordinal: ordinal as u32,
                        byte_len: record.bytes.len() as u64,
                        sha256: cadmpeg_ir::hash::sha256_hex(record.bytes),
                        payload_byte_len: record.payload.len() as u64,
                        payload_sha256: cadmpeg_ir::hash::sha256_hex(record.payload),
                        payload_source_offset: entry_offset + record.payload_offset as u64,
                        source_offset: entry_offset + record.offset as u64,
                    })
                }),
        );
    }
    records
}

/// Decode ordered self-framed strings from feature-operation payloads.
pub fn feature_payload_strings(container: &Container) -> Vec<FeaturePayloadString> {
    let sections = container.om_sections();
    let mut strings = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let operation_record = format!(
                "nx:feature-history:operation-record#{section_key}-{operation_ordinal:010}"
            );
            strings.extend(
                crate::om::operation_payload_strings(record)
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, value)| FeaturePayloadString {
                        id: format!(
                            "nx:feature-history:payload-string#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                        ),
                        operation_record: operation_record.clone(),
                        ordinal: ordinal as u32,
                        value: value.value.to_string(),
                        source_offset: entry_offset + value.offset as u64,
                    }),
            );
        }
    }
    strings
}

/// Join exact `SIMPLE HOLE` payload templates to their operation identities.
pub fn feature_simple_hole_templates(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    strings: &[FeaturePayloadString],
) -> Vec<FeatureSimpleHoleTemplate> {
    let labels_by_id = labels
        .iter()
        .map(|label| (label.id.as_str(), label))
        .collect::<BTreeMap<_, _>>();
    let records_by_id = records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut templates_by_operation = BTreeMap::<String, Vec<_>>::new();
    for string in strings {
        let Some(record) = records_by_id.get(string.operation_record.as_str()) else {
            continue;
        };
        let Some(label) = labels_by_id.get(record.operation_label.as_str()) else {
            continue;
        };
        if label.value != "SIMPLE HOLE" || !string.value.starts_with("Hole_") {
            continue;
        }
        templates_by_operation
            .entry(label.id.clone())
            .or_default()
            .push((string, *label));
    }
    templates_by_operation
        .into_values()
        .filter_map(|candidates| {
            let [(string, label)] = candidates.as_slice() else {
                return None;
            };
            let (family, form, extent, start_treatment, end_treatment) =
                parse_simple_hole_template(&string.value)?;
            Some(FeatureSimpleHoleTemplate {
                id: string
                    .id
                    .replacen("payload-string", "simple-hole-template", 1),
                operation_label: label.id.clone(),
                payload_string: string.id.clone(),
                family,
                form,
                extent,
                start_treatment,
                end_treatment,
            })
        })
        .collect()
}

/// Decode exact nonempty duplicated scalar lanes from simple-hole operations.
pub fn feature_simple_hole_repeated_scalar_lanes(
    container: &Container,
) -> Vec<FeatureSimpleHoleRepeatedScalarLane> {
    let sections = container.om_sections();
    let mut pairs = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(pair) = crate::om::simple_hole_repeated_scalar_lane(record) else {
                continue;
            };
            pairs.push(FeatureSimpleHoleRepeatedScalarLane {
                id: format!(
                    "nx:feature-history:simple-hole-repeated-scalar-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                values: pair.values,
                first_witness_offsets: pair.witness_offsets[0]
                    .iter()
                    .map(|offset| entry_offset + *offset as u64)
                    .collect(),
                second_witness_offsets: pair.witness_offsets[1]
                    .iter()
                    .map(|offset| entry_offset + *offset as u64)
                    .collect(),
            });
        }
    }
    pairs
}

/// Resolve the tagged block-index pairs following both repeated scalar-lane
/// witnesses through the unique offset store that owns the operation inputs.
pub fn feature_simple_hole_repeated_scalar_lane_block_references(
    container: &Container,
) -> Vec<FeatureSimpleHoleRepeatedScalarLaneBlockReferences> {
    let sections = container.om_sections();
    let inputs = feature_input_blocks(container);
    let blocks = data_blocks(container)
        .into_iter()
        .map(|block| block.id)
        .collect::<BTreeSet<_>>();
    let mut references = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let prefixes = inputs
                .iter()
                .filter(|input| input.operation_label == operation_label)
                .filter_map(|input| {
                    input
                        .data_block
                        .rsplit_once(":block#")
                        .map(|(prefix, _)| prefix)
                })
                .collect::<BTreeSet<_>>();
            if prefixes.len() != 1 {
                continue;
            }
            let prefix = prefixes.into_iter().next().expect("one checked prefix");
            let Some(decoded) =
                crate::om::simple_hole_repeated_scalar_lane_block_references(record)
            else {
                continue;
            };
            let resolve = |indices: [u32; 2]| {
                let targets = indices.map(|index| format!("{prefix}:block#{index}"));
                targets
                    .iter()
                    .all(|target| blocks.contains(target))
                    .then_some(targets)
            };
            let (Some(first_data_blocks), Some(second_data_blocks)) =
                (resolve(decoded.first), resolve(decoded.second))
            else {
                continue;
            };
            references.push(FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
                id: format!(
                    "nx:feature-history:simple-hole-repeated-scalar-lane-block-references#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                first_data_blocks,
                second_data_blocks,
                first_reference_offsets: decoded.offsets[0].map(|offset| entry_offset + offset as u64),
                second_reference_offsets: decoded.offsets[1].map(|offset| entry_offset + offset as u64),
            });
        }
    }
    references
}

/// Group distinct simple-hole operations that address the same four construction blocks.
pub fn feature_simple_hole_construction_groups(
    lanes: &[FeatureSimpleHoleRepeatedScalarLane],
    references: &[FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
) -> Vec<FeatureSimpleHoleConstructionGroup> {
    let mut lanes_by_operation = BTreeMap::<&str, Vec<_>>::new();
    for lane in lanes {
        lanes_by_operation
            .entry(lane.operation_label.as_str())
            .or_default()
            .push(lane);
    }
    let mut grouped = BTreeMap::<([String; 2], [String; 2]), Vec<_>>::new();
    let mut ambiguous_groups = BTreeSet::new();
    for reference in references {
        let key = (
            reference.first_data_blocks.clone(),
            reference.second_data_blocks.clone(),
        );
        let lane = match lanes_by_operation
            .get(reference.operation_label.as_str())
            .map(Vec::as_slice)
        {
            Some([lane]) => *lane,
            Some(_) => {
                ambiguous_groups.insert(key);
                continue;
            }
            None => continue,
        };
        grouped.entry(key).or_default().push((reference, lane));
    }
    grouped
        .into_iter()
        .filter_map(|(key, mut members)| {
            if ambiguous_groups.contains(&key) {
                return None;
            }
            members.sort_by(|(first, _), (second, _)| {
                first.operation_label.cmp(&second.operation_label)
            });
            if members.len() < 2
                || members
                    .windows(2)
                    .any(|pair| pair[0].0.operation_label == pair[1].0.operation_label)
            {
                return None;
            }
            let first = members[0].0;
            let key = first
                .operation_label
                .rsplit_once('#')
                .map_or("unknown", |(_, key)| key);
            Some(FeatureSimpleHoleConstructionGroup {
                id: format!("nx:feature-history:simple-hole-construction-group#{key}"),
                first_data_blocks: first.first_data_blocks.clone(),
                second_data_blocks: first.second_data_blocks.clone(),
                operation_labels: members
                    .iter()
                    .map(|(reference, _)| reference.operation_label.clone())
                    .collect(),
                scalar_lanes: members.iter().map(|(_, lane)| lane.id.clone()).collect(),
                block_references: members
                    .iter()
                    .map(|(reference, _)| reference.id.clone())
                    .collect(),
            })
        })
        .collect()
}

pub(crate) fn parse_simple_hole_template(
    value: &str,
) -> Option<(
    SimpleHoleFamily,
    SimpleHoleForm,
    SimpleHoleExtent,
    SimpleHoleEndTreatment,
    SimpleHoleEndTreatment,
)> {
    (value.split('_').collect::<Vec<_>>().as_slice()
        == [
            "Hole",
            "GeneralHole",
            "Simple",
            "Through",
            "StartChamfer",
            "EndChamfer",
        ])
    .then_some((
        SimpleHoleFamily::GeneralHole,
        SimpleHoleForm::Simple,
        SimpleHoleExtent::Through,
        SimpleHoleEndTreatment::Chamfer,
        SimpleHoleEndTreatment::Chamfer,
    ))
}

/// Decode primary body lineage references from feature-history operations.
pub fn feature_body_references(container: &Container) -> Vec<FeatureBodyReference> {
    let sections = container.om_sections();
    let mut references = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, reference) in section.operation_body_references() {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{ordinal:010}");
            references.push(FeatureBodyReference {
                id: format!("nx:feature-history:body-reference#{section_key}-{ordinal:010}"),
                operation_label,
                body_object_index: reference.object_index,
                source_offset: entry_offset + reference.offset as u64,
            });
        }
    }
    references
}

/// Decode every ordered body-reference field from bounded feature operations.
pub fn feature_body_reference_occurrences(
    container: &Container,
) -> Vec<FeatureBodyReferenceOccurrence> {
    let sections = container.om_sections();
    let mut references = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            references.extend(
                crate::om::operation_body_references(record)
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, reference)| FeatureBodyReferenceOccurrence {
                        id: format!(
                            "nx:feature-history:body-reference-occurrence#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                        ),
                        operation_label: operation_label.clone(),
                        ordinal: ordinal as u32,
                        body_object_index: reference.object_index,
                        source_offset: entry_offset + reference.offset as u64,
                    }),
            );
        }
    }
    references
}

/// Join primary feature body fields to exactly one segment body alias pair.
pub fn feature_body_segment_uses(
    references: &[FeatureBodyReference],
    bindings: &[SegmentBodyBinding],
) -> Vec<FeatureBodySegmentUse> {
    references
        .iter()
        .filter_map(|reference| {
            let matches = bindings
                .iter()
                .filter(|binding| {
                    binding.body_object_index == reference.body_object_index
                        || binding.body_alias_object_index == reference.body_object_index
                })
                .collect::<Vec<_>>();
            let [binding] = matches.as_slice() else {
                return None;
            };
            Some(FeatureBodySegmentUse {
                id: reference
                    .id
                    .replacen("body-reference", "body-segment-use", 1),
                feature_body_reference: reference.id.clone(),
                segment_body_binding: binding.id.clone(),
            })
        })
        .collect()
}

/// Return body objects whose latest decoded writer is not consumed by a later
/// Boolean, sewing, or trimming operation. Segment-bound bodies exist before
/// the retained history area unless a decoded operation writes them.
pub fn terminal_feature_body_indices(
    labels: &[FeatureOperationLabel],
    references: &[FeatureBodyReference],
    booleans: &[FeatureBooleanOperation],
    operands: &[FeatureOperationBodyOperand],
    bindings: &[SegmentBodyBinding],
) -> Option<BTreeSet<u32>> {
    if references.is_empty() && bindings.is_empty() {
        return None;
    }
    let positions = labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let aliases = body_alias_roots(bindings)?;
    let canonical = |identity: u32| aliases.get(&identity).copied().unwrap_or(identity);
    let operation_kinds = labels
        .iter()
        .map(|label| (label.id.as_str(), label.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut last_writers = bindings
        .iter()
        .flat_map(|binding| [binding.body_object_index, binding.body_alias_object_index])
        .map(|identity| (canonical(identity), None))
        .collect::<BTreeMap<u32, Option<usize>>>();
    for reference in references {
        let position = *positions.get(reference.operation_label.as_str())?;
        if operation_kinds.get(reference.operation_label.as_str()) == Some(&"DELETE") {
            continue;
        }
        last_writers.insert(canonical(reference.body_object_index), Some(position));
    }
    let mut consumed = BTreeSet::new();
    for operation in booleans {
        let position = *positions.get(operation.operation_label.as_str())?;
        for tool in &operation.tool_object_indices {
            let tool = canonical(*tool);
            if last_writers
                .get(&tool)
                .is_some_and(|writer| writer.is_none_or(|writer| writer < position))
            {
                consumed.insert(tool);
            }
        }
    }
    for reference in references {
        if operation_kinds.get(reference.operation_label.as_str()) == Some(&"DELETE") {
            let position = *positions.get(reference.operation_label.as_str())?;
            let body = canonical(reference.body_object_index);
            if last_writers
                .get(&body)
                .is_some_and(|writer| writer.is_none_or(|writer| writer < position))
            {
                consumed.insert(body);
            }
        }
    }
    for operand in operands {
        if !matches!(
            operation_kinds.get(operand.operation_label.as_str()),
            Some(&("SEW" | "TRIM BODY"))
        ) {
            continue;
        }
        let position = *positions.get(operand.operation_label.as_str())?;
        let body = canonical(operand.operand_object_index);
        if last_writers
            .get(&body)
            .is_some_and(|writer| writer.is_none_or(|writer| writer < position))
        {
            consumed.insert(body);
        }
    }
    let terminal_roots = last_writers
        .into_keys()
        .filter(|body| !consumed.contains(body))
        .collect::<BTreeSet<_>>();
    Some(
        references
            .iter()
            .map(|reference| reference.body_object_index)
            .chain(
                bindings.iter().flat_map(|binding| {
                    [binding.body_object_index, binding.body_alias_object_index]
                }),
            )
            .filter(|identity| terminal_roots.contains(&canonical(*identity)))
            .collect(),
    )
}

/// Resolve one atomic terminal status for every segment-bound body image.
pub fn segment_body_lineage_statuses(
    labels: &[FeatureOperationLabel],
    references: &[FeatureBodyReference],
    booleans: &[FeatureBooleanOperation],
    operands: &[FeatureOperationBodyOperand],
    bindings: &[SegmentBodyBinding],
) -> Option<Vec<SegmentBodyLineageStatus>> {
    let terminal = terminal_feature_body_indices(labels, references, booleans, operands, bindings)?;
    bindings
        .iter()
        .map(|binding| {
            let statuses = [binding.body_object_index, binding.body_alias_object_index]
                .map(|identity| terminal.contains(&identity));
            if statuses[0] != statuses[1] {
                return None;
            }
            let key = binding
                .id
                .rsplit_once('#')
                .map_or(binding.id.as_str(), |(_, key)| key);
            Some(SegmentBodyLineageStatus {
                id: format!("nx:segment-body-lineage:status#{key}"),
                segment_body_binding: binding.id.clone(),
                body_object_index: binding.body_object_index,
                body_alias_object_index: binding.body_alias_object_index,
                terminal: statuses[0],
                source_offset: binding.source_offset,
            })
        })
        .collect()
}

/// Map each segment body identity to the smallest identity in its transitive alias component.
pub(crate) fn body_alias_roots(bindings: &[SegmentBodyBinding]) -> Option<BTreeMap<u32, u32>> {
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    for binding in bindings {
        adjacency
            .entry(binding.body_object_index)
            .or_default()
            .insert(binding.body_alias_object_index);
        adjacency
            .entry(binding.body_alias_object_index)
            .or_default()
            .insert(binding.body_object_index);
    }
    let mut roots = BTreeMap::new();
    for identity in adjacency.keys().copied() {
        if roots.contains_key(&identity) {
            continue;
        }
        let mut component = BTreeSet::new();
        let mut pending = vec![identity];
        while let Some(member) = pending.pop() {
            if !component.insert(member) {
                continue;
            }
            pending.extend(
                adjacency
                    .get(&member)
                    .into_iter()
                    .flatten()
                    .filter(|neighbor| !component.contains(neighbor))
                    .copied(),
            );
        }
        let root = *component.first()?;
        roots.extend(component.into_iter().map(|member| (member, root)));
    }
    Some(roots)
}

/// Resolve operation-header object indices to unique offset-only data blocks.
pub fn feature_input_blocks(container: &Container) -> Vec<FeatureInputBlock> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut inputs = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, label) in section.operation_labels().into_iter().enumerate() {
            for (input_slot, object_index) in label.object_indices.into_iter().enumerate() {
                let Some(object_index) = object_index else {
                    continue;
                };
                let Some(data_block) = unique_offset_data_block(&indexed, object_index) else {
                    continue;
                };
                let operation_label = format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                );
                inputs.push(FeatureInputBlock {
                    id: format!(
                        "nx:feature-history:input-block#{section_key}-{operation_ordinal:010}-{input_slot:010}"
                    ),
                    operation_label,
                    input_slot: input_slot as u8,
                    object_index,
                    data_block,
                    source_offset: entry_offset + label.object_index_offsets[input_slot] as u64,
                });
            }
        }
    }
    inputs
}

/// Group bindings from distinct operations by exact resolved data-block identity.
pub fn feature_input_block_identity_groups(
    inputs: &[FeatureInputBlock],
) -> Vec<FeatureInputBlockIdentityGroup> {
    let mut by_block = BTreeMap::<&str, Vec<&FeatureInputBlock>>::new();
    for input in inputs {
        by_block.entry(&input.data_block).or_default().push(input);
    }
    let mut groups = by_block
        .into_iter()
        .filter_map(|(data_block, mut members)| {
            if members
                .iter()
                .map(|member| member.operation_label.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                < 2
            {
                return None;
            }
            members.sort_by_key(|member| member.source_offset);
            Some((data_block, members))
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|(_, members)| members[0].source_offset);
    groups
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (data_block, members))| FeatureInputBlockIdentityGroup {
                id: format!("nx:feature-history:input-block-identity-group#{ordinal:010}"),
                data_block: data_block.to_string(),
                input_blocks: members.iter().map(|member| member.id.clone()).collect(),
                operation_labels: members
                    .iter()
                    .map(|member| member.operation_label.clone())
                    .collect(),
                input_slots: members.iter().map(|member| member.input_slot).collect(),
                source_offsets: members.iter().map(|member| member.source_offset).collect(),
            },
        )
        .collect()
}

/// Join feature inputs to every column-row slot addressing the same block.
pub fn feature_input_column_row_uses(
    inputs: &[FeatureInputBlock],
    index_rows: &[DataBlockIndexRow],
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
    tables: &[DataBlockColumnIndexTable],
) -> Vec<FeatureInputColumnRowUse> {
    let mut table_by_row = BTreeMap::<&str, Option<&str>>::new();
    for table in tables {
        for row in std::iter::once(table.opening_linked_row.as_str())
            .chain(table.target_rows.iter().map(String::as_str))
            .chain(table.linked_rows.iter().map(String::as_str))
        {
            table_by_row
                .entry(row)
                .and_modify(|value| *value = None)
                .or_insert(Some(table.id.as_str()));
        }
    }
    let mut slots_by_block = BTreeMap::<&str, Vec<(&str, ColumnIndexRowKind, usize, u64)>>::new();
    for row in index_rows {
        for (slot, data_block) in row.data_blocks.iter().enumerate() {
            slots_by_block.entry(data_block).or_default().push((
                row.id.as_str(),
                ColumnIndexRowKind::Index,
                slot,
                row.index_source_offsets[slot],
            ));
        }
    }
    for row in linked_rows {
        for (slot, data_block) in row.data_blocks.iter().enumerate() {
            slots_by_block.entry(data_block).or_default().push((
                row.id.as_str(),
                ColumnIndexRowKind::LinkedIndex,
                slot,
                if slot == 0 {
                    row.target_index_source_offset
                } else {
                    row.index_source_offsets[slot - 1]
                },
            ));
        }
    }
    for row in target_rows {
        for (slot, data_block) in row.data_blocks.iter().enumerate() {
            slots_by_block.entry(data_block).or_default().push((
                row.id.as_str(),
                ColumnIndexRowKind::TargetIndex,
                slot,
                if slot == 0 {
                    row.target_index_source_offset
                } else {
                    row.index_source_offsets[slot - 1]
                },
            ));
        }
    }
    inputs
        .iter()
        .flat_map(|input| {
            slots_by_block
                .get(input.data_block.as_str())
                .into_iter()
                .flatten()
                .enumerate()
                .map(
                    |(ordinal, (row, row_kind, slot, source_offset))| FeatureInputColumnRowUse {
                        id: format!(
                            "nx:feature-history:input-column-row-use#{}-{}-{ordinal:010}",
                            input.id.rsplit_once('#').map_or("unknown", |(_, key)| key),
                            row_kind.id_component(),
                        ),
                        input_block: input.id.clone(),
                        operation_label: input.operation_label.clone(),
                        input_slot: input.input_slot,
                        row_kind: *row_kind,
                        column_row: (*row).to_string(),
                        column_table: table_by_row
                            .get(row)
                            .and_then(|table| *table)
                            .map(str::to_string),
                        row_slot: *slot as u8,
                        data_block: input.data_block.clone(),
                        source_offset: *source_offset,
                    },
                )
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Retain inputs having exactly one slot-zero use in one complete column table.
pub fn feature_input_column_targets(
    inputs: &[FeatureInputBlock],
    uses: &[FeatureInputColumnRowUse],
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
) -> Vec<FeatureInputColumnTarget> {
    inputs
        .iter()
        .filter_map(|input| {
            let targets = uses
                .iter()
                .filter(|use_| {
                    use_.input_block == input.id
                        && use_.row_slot == 0
                        && use_.column_table.is_some()
                        && use_.row_kind != ColumnIndexRowKind::Index
                })
                .collect::<Vec<_>>();
            let [target] = targets.as_slice() else {
                return None;
            };
            let (
                leading_index,
                leading_index_source_offset,
                discriminator,
                field_indices,
                field_data_blocks,
                field_source_offsets,
                flag,
                mode,
            ) = match target.row_kind {
                ColumnIndexRowKind::LinkedIndex => {
                    let rows = linked_rows
                        .iter()
                        .filter(|row| row.id == target.column_row)
                        .collect::<Vec<_>>();
                    let [row] = rows.as_slice() else {
                        return None;
                    };
                    (
                        Some(row.first_index),
                        Some(row.first_index_source_offset),
                        Some(row.discriminator),
                        row.indices,
                        std::array::from_fn(|index| row.data_blocks[index + 1].clone()),
                        row.index_source_offsets,
                        Some(row.flag),
                        row.mode,
                    )
                }
                ColumnIndexRowKind::TargetIndex => {
                    let rows = target_rows
                        .iter()
                        .filter(|row| row.id == target.column_row)
                        .collect::<Vec<_>>();
                    let [row] = rows.as_slice() else {
                        return None;
                    };
                    (
                        None,
                        None,
                        None,
                        row.indices,
                        std::array::from_fn(|index| row.data_blocks[index + 1].clone()),
                        row.index_source_offsets,
                        None,
                        row.mode,
                    )
                }
                ColumnIndexRowKind::Index => return None,
            };
            Some(FeatureInputColumnTarget {
                id: format!(
                    "nx:feature-history:input-column-target#{}",
                    input.id.rsplit_once('#').map_or("unknown", |(_, key)| key)
                ),
                input_block: input.id.clone(),
                operation_label: input.operation_label.clone(),
                input_slot: input.input_slot,
                column_row: target.column_row.clone(),
                row_kind: target.row_kind,
                leading_index,
                leading_index_source_offset,
                discriminator,
                field_indices,
                field_data_blocks,
                field_source_offsets,
                flag,
                mode,
                column_table: target
                    .column_table
                    .clone()
                    .expect("complete target use has a table"),
                data_block: input.data_block.clone(),
                source_offset: target.source_offset,
            })
        })
        .collect()
}

/// Decode and atomically resolve datum coordinate-system construction lanes
/// through the offset store selected by each operation header.
pub fn feature_datum_csys_constructions(
    container: &Container,
) -> Vec<FeatureDatumCsysConstruction> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let inputs = feature_input_blocks(container);
    let mut constructions = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(field) = crate::om::datum_csys_references(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let input_prefixes = inputs
                .iter()
                .filter(|input| input.operation_label == operation_label)
                .filter_map(|input| {
                    input
                        .data_block
                        .rsplit_once(":block#")
                        .map(|(prefix, _)| prefix)
                })
                .collect::<BTreeSet<_>>();
            if input_prefixes.len() != 1 {
                continue;
            }
            let input_prefix = input_prefixes
                .into_iter()
                .next()
                .expect("one checked input store");
            let resolved = field.references.map(|reference| {
                unique_offset_data_block(&indexed, reference.object_index).map(|data_block| {
                    (
                        reference.object_index,
                        data_block,
                        entry_offset + reference.offset as u64,
                    )
                })
            });
            let Some(resolved) = resolved.into_iter().collect::<Option<Vec<_>>>() else {
                continue;
            };
            if resolved.iter().any(|(_, data_block, _)| {
                data_block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != input_prefix)
            }) {
                continue;
            }
            constructions.push(FeatureDatumCsysConstruction {
                id: format!(
                    "nx:feature-history:datum-csys-construction#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                control: field.control,
                object_indices: resolved
                    .iter()
                    .map(|(object_index, _, _)| *object_index)
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("eight decoded references"),
                data_blocks: resolved
                    .iter()
                    .map(|(_, data_block, _)| data_block.clone())
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("eight decoded references"),
                source_offsets: resolved
                    .iter()
                    .map(|(_, _, source_offset)| *source_offset)
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("eight decoded references"),
            });
        }
    }
    constructions
}

/// Decode common datum-plane payload headers from feature-history records.
pub fn feature_datum_plane_headers(container: &Container) -> Vec<FeatureDatumPlaneHeader> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let inputs = feature_input_blocks(container);
    let mut headers = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(header) = crate::om::datum_plane_payload_header(record) else {
                continue;
            };
            let single = crate::om::datum_plane_descriptor_reference_branch(record);
            let double = crate::om::datum_plane_double_reference_branch(record);
            let descriptor_indices = single
                .iter()
                .map(|branch| branch.descriptor_index)
                .collect::<Vec<_>>();
            let object_indices = single
                .iter()
                .map(|branch| branch.object_index)
                .chain(double.iter().flat_map(|branch| {
                    branch
                        .references
                        .iter()
                        .map(|reference| reference.object_index)
                }))
                .collect::<Vec<_>>();
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let input_prefixes = inputs
                .iter()
                .filter(|input| input.operation_label == operation_label)
                .filter_map(|input| {
                    input
                        .data_block
                        .rsplit_once(":block#")
                        .map(|(prefix, _)| prefix)
                })
                .collect::<BTreeSet<_>>();
            let resolved = (object_indices.len() + descriptor_indices.len() > 0)
                .then_some(())
                .and_then(|()| {
                    if input_prefixes.len() != 1 {
                        return None;
                    }
                    let input_prefix = *input_prefixes.iter().next()?;
                    let descriptor_blocks = descriptor_indices
                        .iter()
                        .map(|index| unique_offset_data_block(&indexed, *index))
                        .collect::<Option<Vec<_>>>()?;
                    let object_blocks = object_indices
                        .iter()
                        .map(|index| unique_offset_data_block(&indexed, *index))
                        .collect::<Option<Vec<_>>>()?;
                    let same_store = |block: &str| {
                        block
                            .rsplit_once(":block#")
                            .is_some_and(|(prefix, _)| prefix == input_prefix)
                    };
                    descriptor_blocks
                        .iter()
                        .chain(&object_blocks)
                        .all(|block| same_store(block))
                        .then_some((descriptor_blocks, object_blocks))
                });
            headers.push(FeatureDatumPlaneHeader {
                id: format!(
                    "nx:feature-history:datum-plane-header#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                control: header.control,
                declared_count: header.declared_count,
                branch_tag: header.branch_tag,
                descriptor_indices,
                object_indices,
                descriptor_data_blocks: resolved
                    .as_ref()
                    .map_or_else(Vec::new, |(blocks, _)| blocks.clone()),
                object_data_blocks: resolved
                    .as_ref()
                    .map_or_else(Vec::new, |(_, blocks)| blocks.clone()),
                descriptor_source_offsets: single
                    .iter()
                    .map(|branch| entry_offset + branch.descriptor_offset as u64)
                    .collect(),
                object_source_offsets: single
                    .iter()
                    .map(|branch| entry_offset + branch.object_offset as u64)
                    .chain(double.iter().flat_map(|branch| {
                        branch
                            .references
                            .iter()
                            .map(|reference| entry_offset + reference.offset as u64)
                    }))
                    .collect(),
                source_offset: entry_offset + record.payload_offset as u64,
            });
        }
    }
    headers
}

/// Reconstruct datum-plane object payloads across ordered store blocks.
pub fn feature_datum_plane_payloads(
    container: &Container,
    headers: &[FeatureDatumPlaneHeader],
) -> Vec<FeatureDatumPlanePayload> {
    let blocks = offset_data_block_bytes(container);
    headers
        .iter()
        .filter(|header| !header.object_data_blocks.is_empty())
        .filter_map(|header| {
            let (payload, block_payload_offsets, block_byte_lengths, block_source_offsets) =
                join_data_block_bytes(&header.object_data_blocks, &blocks)?;
            let lanes = crate::om::datum_plane_object_index_lanes(&payload);
            let lane = match lanes.as_slice() {
                [lane] => Some(lane),
                _ => None,
            };
            let key = header.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
            Some(FeatureDatumPlanePayload {
                id: format!("nx:feature-history:datum-plane-payload#{key}"),
                operation_label: header.operation_label.clone(),
                datum_plane_header: header.id.clone(),
                data_blocks: header.object_data_blocks.clone(),
                byte_len: payload.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&payload),
                block_payload_offsets,
                block_byte_lengths,
                block_source_offsets,
                index_lane_offset: lane.map(|lane| lane.offset as u64),
                index_lane_declared_count: lane.map(|lane| lane.declared_count),
                index_lane_values: lane.map_or_else(Vec::new, |lane| {
                    lane.indices.iter().map(|(value, _)| *value).collect()
                }),
                index_lane_value_offsets: lane.map_or_else(Vec::new, |lane| {
                    lane.indices
                        .iter()
                        .map(|(_, offset)| *offset as u64)
                        .collect()
                }),
                index_lane_trailer: lane.map(|lane| lane.trailer),
            })
        })
        .collect()
}

/// Reconstruct the two leading object blocks of each datum coordinate system.
pub fn feature_datum_csys_payloads(
    container: &Container,
    constructions: &[FeatureDatumCsysConstruction],
) -> Vec<FeatureDatumCsysPayload> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .filter_map(|construction| {
            let data_blocks = [
                construction.data_blocks[0].clone(),
                construction.data_blocks[1].clone(),
            ];
            let (bytes, starts, lengths, sources) = join_data_block_bytes(&data_blocks, &blocks)?;
            Some(FeatureDatumCsysPayload {
                id: construction
                    .id
                    .replacen("datum-csys-construction", "datum-csys-payload", 1),
                operation_label: construction.operation_label.clone(),
                construction: construction.id.clone(),
                data_blocks,
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&bytes),
                block_payload_offsets: starts.try_into().ok()?,
                block_byte_lengths: lengths.try_into().ok()?,
                block_source_offsets: sources.try_into().ok()?,
            })
        })
        .collect()
}

/// Decode exact scalar-pair frames from reconstructed datum-CSYS payloads.
pub fn feature_datum_csys_payload_scalar_pairs(
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Vec<FeatureDatumCsysPayloadScalarPair> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let source_offset =
                |relative: usize| {
                    let relative = relative as u64;
                    starts.iter().zip(&lengths).zip(&sources).find_map(
                        |((start, length), source)| {
                            (relative >= *start && relative < start.saturating_add(*length))
                                .then_some(source + relative - start)
                        },
                    )
                };
            crate::om::object_payload_scalar_pairs(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeatureDatumCsysPayloadScalarPair {
                        id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        datum_csys_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        payload_offset: pair.offset as u64,
                        value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                        source_offset: source_offset(pair.offset)?,
                        value_source_offsets: [
                            source_offset(pair.value_offsets[0])?,
                            source_offset(pair.value_offsets[1])?,
                        ],
                        discriminator: pair.discriminator,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode complete signed Q1.55 pair frames from reconstructed datum-CSYS payloads.
pub fn feature_datum_csys_payload_fixed_pairs(
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Vec<FeatureDatumCsysPayloadFixedPair> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let source_offset =
                |relative: usize| {
                    let relative = relative as u64;
                    starts.iter().zip(&lengths).zip(&sources).find_map(
                        |((start, length), source)| {
                            (relative >= *start && relative < start.saturating_add(*length))
                                .then_some(source + relative - start)
                        },
                    )
                };
            crate::om::datum_csys_payload_fixed_pairs(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeatureDatumCsysPayloadFixedPair {
                        id: format!("{}-fixed-pair-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        datum_csys_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        raw_values: pair.raw_values,
                        discriminator: pair.discriminator,
                        payload_offset: pair.offset as u64,
                        value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                        source_offset: source_offset(pair.offset)?,
                        value_source_offsets: [
                            source_offset(pair.value_offsets[0])?,
                            source_offset(pair.value_offsets[1])?,
                        ],
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode complete shifted-binary64 fields from reconstructed datum-CSYS payloads.
pub fn feature_datum_csys_payload_scalars(
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Vec<FeatureDatumCsysPayloadScalar> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let source_offset =
                |relative: usize| {
                    let relative = relative as u64;
                    starts.iter().zip(&lengths).zip(&sources).find_map(
                        |((start, length), source)| {
                            (relative >= *start && relative < start.saturating_add(*length))
                                .then_some(source + relative - start)
                        },
                    )
                };
            crate::om::construction_payload_scalar_fields(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, scalar)| {
                    Some(FeatureDatumCsysPayloadScalar {
                        id: format!("{}-scalar-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        datum_csys_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        field_code: scalar.field_code,
                        value: scalar.value,
                        payload_offset: scalar.offset as u64,
                        source_offset: source_offset(scalar.offset)?,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode the final three descriptor lanes of datum coordinate systems.
pub fn feature_datum_csys_descriptors(
    container: &Container,
    constructions: &[FeatureDatumCsysConstruction],
) -> Vec<FeatureDatumCsysDescriptor> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .flat_map(|construction| {
            (5..8)
                .filter_map(|reference_ordinal| {
                    let data_block = &construction.data_blocks[reference_ordinal];
                    let &(bytes, source_offset) = blocks.get(data_block)?;
                    let descriptor = crate::om::datum_csys_descriptor_block(bytes)?;
                    Some(FeatureDatumCsysDescriptor {
                        id: format!("{}-descriptor-{reference_ordinal}", construction.id),
                        operation_label: construction.operation_label.clone(),
                        construction: construction.id.clone(),
                        reference_ordinal: reference_ordinal as u8,
                        data_block: data_block.clone(),
                        prefix: descriptor.prefix,
                        identity: descriptor.identity,
                        suffix: descriptor.suffix,
                        source_offset,
                        identity_source_offset: source_offset + descriptor.identity_offset as u64,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join equal typed descriptor identities across datum-plane and datum-CSYS history.
pub fn feature_datum_plane_csys_identity_uses(
    plane_descriptors: &[FeatureDatumPlaneDescriptor],
    csys_descriptors: &[FeatureDatumCsysDescriptor],
) -> Vec<FeatureDatumPlaneCsysIdentityUse> {
    plane_descriptors
        .iter()
        .flat_map(|plane| {
            csys_descriptors
                .iter()
                .filter(|csys| csys.identity == plane.identity)
                .map(|csys| {
                    let plane_key = plane.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
                    let csys_key = csys.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
                    FeatureDatumPlaneCsysIdentityUse {
                        id: format!(
                            "nx:feature-history:datum-plane-csys-identity-use#{plane_key}-{csys_key}"
                        ),
                        identity: plane.identity.clone(),
                        datum_plane_descriptor: plane.id.clone(),
                        datum_plane_operation_label: plane.operation_label.clone(),
                        datum_csys_descriptor: csys.id.clone(),
                        datum_csys_operation_label: csys.operation_label.clone(),
                        datum_csys_reference_ordinal: csys.reference_ordinal,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode exact scalar-pair frames from reconstructed datum-plane payloads.
pub fn feature_datum_plane_payload_scalar_pairs(
    container: &Container,
    payloads: &[FeatureDatumPlanePayload],
) -> Vec<FeatureDatumPlanePayloadScalarPair> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let source_offset =
                |relative: usize| {
                    let relative = relative as u64;
                    starts.iter().zip(&lengths).zip(&sources).find_map(
                        |((start, length), source)| {
                            (relative >= *start && relative < start.saturating_add(*length))
                                .then_some(source + relative - start)
                        },
                    )
                };
            crate::om::datum_plane_object_scalar_pairs(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeatureDatumPlanePayloadScalarPair {
                        id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        datum_plane_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        payload_offset: pair.offset as u64,
                        value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                        source_offset: source_offset(pair.offset)?,
                        value_source_offsets: [
                            source_offset(pair.value_offsets[0])?,
                            source_offset(pair.value_offsets[1])?,
                        ],
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode atomically resolved datum-plane descriptor blocks.
pub fn feature_datum_plane_descriptors(
    container: &Container,
    headers: &[FeatureDatumPlaneHeader],
) -> Vec<FeatureDatumPlaneDescriptor> {
    let blocks = offset_data_block_bytes(container);
    headers
        .iter()
        .flat_map(|header| {
            header
                .descriptor_data_blocks
                .iter()
                .enumerate()
                .filter_map(|(ordinal, data_block)| {
                    let (bytes, source_offset) = blocks.get(data_block)?.to_owned();
                    let descriptor = crate::om::datum_plane_descriptor_block(bytes)?;
                    Some(FeatureDatumPlaneDescriptor {
                        id: format!("{}-descriptor-{ordinal:010}", header.id),
                        operation_label: header.operation_label.clone(),
                        datum_plane_header: header.id.clone(),
                        ordinal: ordinal as u32,
                        data_block: data_block.clone(),
                        identity: descriptor.identity,
                        suffix: descriptor.suffix,
                        schema_index: descriptor.schema_index,
                        label: descriptor.label,
                        source_offset,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join resolved datum-plane blocks to operation inputs addressing the same block.
pub fn feature_datum_plane_block_uses(
    headers: &[FeatureDatumPlaneHeader],
    inputs: &[FeatureInputBlock],
) -> Vec<FeatureDatumPlaneBlockUse> {
    let mut uses = Vec::new();
    for header in headers {
        let construction_key = header
            .operation_label
            .rsplit_once('#')
            .map_or(header.operation_label.as_str(), |(_, key)| key);
        for (lane, blocks) in [
            (
                DatumPlaneBlockLane::Descriptor,
                &header.descriptor_data_blocks,
            ),
            (DatumPlaneBlockLane::Object, &header.object_data_blocks),
        ] {
            for (reference_ordinal, data_block) in blocks.iter().enumerate() {
                for input in inputs
                    .iter()
                    .filter(|input| input.data_block == *data_block)
                {
                    let input_key = input
                        .operation_label
                        .rsplit_once('#')
                        .map_or(input.operation_label.as_str(), |(_, key)| key);
                    let lane_key = match lane {
                        DatumPlaneBlockLane::Descriptor => "descriptor",
                        DatumPlaneBlockLane::Object => "object",
                    };
                    uses.push(FeatureDatumPlaneBlockUse {
                        id: format!(
                            "nx:feature-history:datum-plane-block-use#{construction_key}-{lane_key}-{reference_ordinal}-{input_key}-{}",
                            input.input_slot
                        ),
                        datum_plane_header: header.id.clone(),
                        construction_operation_label: header.operation_label.clone(),
                        lane,
                        reference_ordinal: reference_ordinal as u32,
                        data_block: data_block.clone(),
                        input_binding: input.id.clone(),
                        input_operation_label: input.operation_label.clone(),
                        input_slot: input.input_slot,
                    });
                }
            }
        }
    }
    uses
}

/// Join resolved datum-coordinate-system blocks to every exact operation input
/// addressing the same native block.
pub fn feature_datum_csys_block_uses(
    constructions: &[FeatureDatumCsysConstruction],
    inputs: &[FeatureInputBlock],
) -> Vec<FeatureDatumCsysBlockUse> {
    let mut uses = Vec::new();
    for construction in constructions {
        for (reference_ordinal, data_block) in construction.data_blocks.iter().enumerate() {
            for input in inputs
                .iter()
                .filter(|input| input.data_block == *data_block)
            {
                let construction_key = construction
                    .operation_label
                    .rsplit_once('#')
                    .map_or(construction.operation_label.as_str(), |(_, key)| key);
                let input_key = input
                    .operation_label
                    .rsplit_once('#')
                    .map_or(input.operation_label.as_str(), |(_, key)| key);
                uses.push(FeatureDatumCsysBlockUse {
                    id: format!(
                        "nx:feature-history:datum-csys-block-use#{construction_key}-{reference_ordinal}-{input_key}-{}",
                        input.input_slot
                    ),
                    construction: construction.id.clone(),
                    construction_operation_label: construction.operation_label.clone(),
                    reference_ordinal: reference_ordinal as u8,
                    data_block: data_block.clone(),
                    input_binding: input.id.clone(),
                    input_operation_label: input.operation_label.clone(),
                    input_slot: input.input_slot,
                });
            }
        }
    }
    uses
}

/// Join each sketch operation to its bounded record and ordered input blocks.
pub fn feature_sketch_records(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    inputs: &[FeatureInputBlock],
    references: &[FeatureSketchReference],
) -> Vec<FeatureSketchRecord> {
    labels
        .iter()
        .filter(|label| label.value == "SKETCH")
        .filter_map(|label| {
            let mut operation_records = records
                .iter()
                .filter(|record| record.operation_label == label.id);
            let record = operation_records.next()?;
            if operation_records.next().is_some() {
                return None;
            }
            let mut input_blocks = inputs
                .iter()
                .filter(|input| input.operation_label == label.id)
                .collect::<Vec<_>>();
            input_blocks.sort_by_key(|input| input.input_slot);
            let mut payload_references = references
                .iter()
                .filter(|reference| reference.operation_label == label.id)
                .collect::<Vec<_>>();
            payload_references.sort_by_key(|reference| reference.ordinal);
            Some(FeatureSketchRecord {
                id: label.id.replacen("operation-label", "sketch-record", 1),
                operation_label: label.id.clone(),
                ordinal: label.ordinal,
                operation_record: record.id.clone(),
                input_blocks: input_blocks
                    .into_iter()
                    .map(|input| input.id.clone())
                    .collect(),
                payload_references: payload_references
                    .into_iter()
                    .map(|reference| reference.id.clone())
                    .collect(),
                source_offset: label.source_offset,
            })
        })
        .collect()
}

/// Join complete, uniquely resolved sketch construction-reference fields.
pub fn feature_sketch_construction_inputs(
    sketches: &[FeatureSketchRecord],
    references: &[FeatureSketchReference],
) -> Vec<FeatureSketchConstructionInputs> {
    let mut inputs = Vec::new();
    for sketch in sketches {
        let mut field = references
            .iter()
            .filter(|reference| reference.operation_label == sketch.operation_label)
            .collect::<Vec<_>>();
        field.sort_by_key(|reference| reference.ordinal);
        let Some(first) = field.first() else {
            continue;
        };
        let expected_len = usize::from(first.declared_count.max(1));
        if field.len() != expected_len
            || field.iter().enumerate().any(|(ordinal, reference)| {
                reference.declared_count != first.declared_count
                    || reference.ordinal != ordinal as u32
                    || reference.terminal != (ordinal + 1 == expected_len)
            })
        {
            continue;
        }
        let Some((terminal, members)) = field.split_last() else {
            continue;
        };
        let Some(member_data_blocks) = members
            .iter()
            .map(|reference| reference.data_block.clone())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(terminal_data_block) = terminal.data_block.clone() else {
            continue;
        };
        inputs.push(FeatureSketchConstructionInputs {
            id: sketch
                .id
                .replacen("sketch-record", "sketch-construction-inputs", 1),
            operation_label: sketch.operation_label.clone(),
            sketch_record: sketch.id.clone(),
            member_references: members
                .iter()
                .map(|reference| reference.id.clone())
                .collect(),
            member_data_blocks,
            terminal_reference: terminal.id.clone(),
            terminal_data_block,
        });
    }
    inputs
}

/// Reconstruct exact sketch payloads across offset-store block boundaries.
pub fn feature_sketch_construction_payloads(
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Vec<FeatureSketchConstructionPayload> {
    let blocks = offset_data_block_bytes(container);

    constructions
        .iter()
        .filter_map(|construction| {
            let mut data_blocks = construction.member_data_blocks.clone();
            data_blocks.push(construction.terminal_data_block.clone());
            let (payload, block_payload_offsets, block_byte_lengths, block_source_offsets) =
                join_data_block_bytes(&data_blocks, &blocks)?;
            Some(FeatureSketchConstructionPayload {
                id: construction.id.replacen(
                    "sketch-construction-inputs",
                    "sketch-construction-payload",
                    1,
                ),
                operation_label: construction.operation_label.clone(),
                construction_inputs: construction.id.clone(),
                data_blocks,
                byte_len: payload.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&payload),
                block_payload_offsets,
                block_byte_lengths,
                block_source_offsets,
            })
        })
        .collect()
}

/// Decode exact coordinate-pair frames from reconstructed sketch payloads.
pub fn feature_sketch_payload_coordinate_pairs(
    container: &Container,
    payloads: &[FeatureSketchConstructionPayload],
) -> Vec<FeatureSketchPayloadCoordinatePair> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let source_offset =
                |relative: usize| {
                    let relative = relative as u64;
                    starts.iter().zip(&lengths).zip(&sources).find_map(
                        |((start, length), source)| {
                            (relative >= *start && relative < start.saturating_add(*length))
                                .then_some(source + relative - start)
                        },
                    )
                };
            crate::om::object_payload_scalar_pairs(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeatureSketchPayloadCoordinatePair {
                        id: format!("{}-coordinate-pair-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        payload_offset: pair.offset as u64,
                        value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                        source_offset: source_offset(pair.offset)?,
                        value_source_offsets: [
                            source_offset(pair.value_offsets[0])?,
                            source_offset(pair.value_offsets[1])?,
                        ],
                        discriminator: pair.discriminator,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode exact signed fixed-point pair frames from reconstructed sketch payloads.
pub fn feature_sketch_payload_fixed_pairs(
    container: &Container,
    payloads: &[FeatureSketchConstructionPayload],
) -> Vec<FeatureSketchPayloadFixedPair> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let source_offset =
                |relative: usize| {
                    let relative = relative as u64;
                    starts.iter().zip(&lengths).zip(&sources).find_map(
                        |((start, length), source)| {
                            (relative >= *start && relative < start.saturating_add(*length))
                                .then_some(source + relative - start)
                        },
                    )
                };
            crate::om::sketch_payload_fixed_pairs(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeatureSketchPayloadFixedPair {
                        id: format!("{}-fixed-pair-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        raw_values: pair.raw_values,
                        payload_offset: pair.offset as u64,
                        value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                        source_offset: source_offset(pair.offset)?,
                        value_source_offsets: [
                            source_offset(pair.value_offsets[0])?,
                            source_offset(pair.value_offsets[1])?,
                        ],
                    })
                })
                .collect()
        })
        .collect()
}

pub(crate) fn offset_data_block_bytes_for_section<'a>(
    section_ordinal: usize,
    entry_offset: u64,
    control: Option<&crate::om::EntityRecord<'a>>,
    records: &[crate::om::EntityRecord<'a>],
) -> BTreeMap<String, (&'a [u8], u64)> {
    let mut blocks = BTreeMap::new();
    let first_record_ordinal = usize::from(control.is_some());
    if let Some(control) = control {
        blocks.insert(
            format!("nx:om-data-blocks-{section_ordinal}:block#0"),
            (control.bytes, entry_offset + control.offset as u64),
        );
    }
    for (record_ordinal, block) in records.iter().enumerate() {
        blocks.insert(
            format!(
                "nx:om-data-blocks-{section_ordinal}:block#{}",
                record_ordinal + first_record_ordinal
            ),
            (block.bytes, entry_offset + block.offset as u64),
        );
    }
    blocks
}

fn offset_data_block_bytes(container: &Container) -> BTreeMap<String, (&[u8], u64)> {
    let mut blocks = BTreeMap::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        if section
            .records
            .first()
            .is_none_or(|record| record.object_id.is_some())
        {
            continue;
        }
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        blocks.extend(offset_data_block_bytes_for_section(
            section_ordinal,
            entry_offset,
            section.control.as_ref(),
            &section.records,
        ));
    }
    blocks
}

/// Decode exact framed scalar fields across reconstructed sketch payloads.
pub fn feature_sketch_payload_scalars(
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Vec<FeatureSketchPayloadScalar> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .filter_map(|construction| {
            let mut data_blocks = construction.member_data_blocks.clone();
            data_blocks.push(construction.terminal_data_block.clone());
            let (payload, block_payload_offsets, block_byte_lengths, block_source_offsets) =
                join_data_block_bytes(&data_blocks, &blocks)?;
            let construction_payload = construction.id.replacen(
                "sketch-construction-inputs",
                "sketch-construction-payload",
                1,
            );
            Some(
                crate::om::construction_payload_scalar_fields(&payload)
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, field)| {
                        let source_offset = block_payload_offsets
                            .iter()
                            .zip(&block_byte_lengths)
                            .zip(&block_source_offsets)
                            .find_map(|((payload_start, byte_len), source_start)| {
                                let relative = u64::try_from(field.offset).ok()?;
                                (relative >= *payload_start
                                    && relative < payload_start.saturating_add(*byte_len))
                                .then_some(source_start + relative - payload_start)
                            })
                            .expect("field lies in joined payload");
                        FeatureSketchPayloadScalar {
                            id: format!(
                                "nx:feature-history:sketch-payload-scalar#{}-{ordinal:010}",
                                construction_payload
                                    .rsplit_once('#')
                                    .map_or("unknown", |(_, key)| key)
                            ),
                            operation_label: construction.operation_label.clone(),
                            construction_payload: construction_payload.clone(),
                            ordinal: ordinal as u32,
                            field_code: field.field_code,
                            value: field.value,
                            payload_offset: field.offset as u64,
                            source_offset,
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
}

/// Decode exact compact-code name fields across reconstructed sketch payloads.
pub fn feature_sketch_payload_names(
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Vec<FeatureSketchPayloadName> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .flat_map(|construction| {
            let mut data_blocks = construction.member_data_blocks.clone();
            data_blocks.push(construction.terminal_data_block.clone());
            let Some((payload, block_payload_offsets, block_byte_lengths, block_source_offsets)) =
                join_data_block_bytes(&data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let construction_payload = construction.id.replacen(
                "sketch-construction-inputs",
                "sketch-construction-payload",
                1,
            );
            crate::om::construction_payload_named_fields(&payload)
                .into_iter()
                .enumerate()
                .map(|(ordinal, field)| {
                    let relative = field.offset as u64;
                    let source_offset = block_payload_offsets
                        .iter()
                        .zip(&block_byte_lengths)
                        .zip(&block_source_offsets)
                        .find_map(|((payload_start, byte_len), source_start)| {
                            (relative >= *payload_start
                                && relative < payload_start.saturating_add(*byte_len))
                            .then_some(source_start + relative - payload_start)
                        })
                        .expect("field lies in joined payload");
                    FeatureSketchPayloadName {
                        id: format!(
                            "nx:feature-history:sketch-payload-name#{}-{ordinal:010}",
                            construction_payload
                                .rsplit_once('#')
                                .map_or("unknown", |(_, key)| key)
                        ),
                        operation_label: construction.operation_label.clone(),
                        construction_payload: construction_payload.clone(),
                        ordinal: ordinal as u32,
                        type_code: field.type_code,
                        payload_leading: field.payload_leading,
                        value: field.value.to_string(),
                        payload_offset: relative,
                        source_offset,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join complete name-delimited intervals to their framed scalar fields.
pub fn feature_sketch_payload_named_records(
    payloads: &[FeatureSketchConstructionPayload],
    names: &[FeatureSketchPayloadName],
    scalars: &[FeatureSketchPayloadScalar],
    fixed_pairs: &[FeatureSketchPayloadFixedPair],
) -> Vec<FeatureSketchPayloadNamedRecord> {
    let mut records = Vec::new();
    for payload in payloads {
        let mut payload_names = names
            .iter()
            .filter(|name| name.construction_payload == payload.id)
            .collect::<Vec<_>>();
        payload_names.sort_by_key(|name| name.payload_offset);
        for (ordinal, name) in payload_names.iter().enumerate() {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload.byte_len, |next| next.payload_offset);
            let mut scalar_fields = scalars
                .iter()
                .filter(|scalar| {
                    scalar.construction_payload == payload.id
                        && scalar.payload_offset > name.payload_offset
                        && scalar.payload_offset < end
                })
                .collect::<Vec<_>>();
            scalar_fields.sort_by_key(|scalar| scalar.payload_offset);
            let mut record_fixed_pairs = fixed_pairs
                .iter()
                .filter(|pair| {
                    pair.construction_payload == payload.id
                        && pair.payload_offset > name.payload_offset
                        && pair.payload_offset < end
                })
                .collect::<Vec<_>>();
            record_fixed_pairs.sort_by_key(|pair| pair.payload_offset);
            records.push(FeatureSketchPayloadNamedRecord {
                id: format!(
                    "nx:feature-history:sketch-payload-record#{}-{ordinal:010}",
                    payload
                        .id
                        .rsplit_once('#')
                        .map_or("unknown", |(_, key)| key)
                ),
                operation_label: payload.operation_label.clone(),
                construction_payload: payload.id.clone(),
                name_field: name.id.clone(),
                scalar_fields: scalar_fields
                    .into_iter()
                    .map(|scalar| scalar.id.clone())
                    .collect(),
                fixed_pairs: record_fixed_pairs
                    .into_iter()
                    .map(|pair| pair.id.clone())
                    .collect(),
                payload_start_offset: name.payload_offset,
                payload_end_offset: end,
            });
        }
    }
    records
}

/// Decode complete `Point<decimal>` records with exactly two scalar fields.
pub fn feature_sketch_points(
    records: &[FeatureSketchPayloadNamedRecord],
    names: &[FeatureSketchPayloadName],
    scalars: &[FeatureSketchPayloadScalar],
) -> Vec<FeatureSketchPoint> {
    let names = names
        .iter()
        .map(|name| (name.id.as_str(), name))
        .collect::<BTreeMap<_, _>>();
    let scalars = scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<BTreeMap<_, _>>();
    records
        .iter()
        .filter_map(|record| {
            let name = names.get(record.name_field.as_str())?;
            parse_sketch_point_name(&name.value)?;
            let [first_id, second_id] = record.scalar_fields.as_slice() else {
                return None;
            };
            let first = scalars.get(first_id.as_str())?;
            let second = scalars.get(second_id.as_str())?;
            Some(FeatureSketchPoint {
                id: format!(
                    "nx:feature-history:sketch-point#{}",
                    record.id.rsplit_once('#').map_or("unknown", |(_, key)| key)
                ),
                operation_label: record.operation_label.clone(),
                named_record: record.id.clone(),
                name: name.value.clone(),
                scalar_fields: [first.id.clone(), second.id.clone()],
                coordinates: [first.value, second.value],
            })
        })
        .collect()
}

/// Decode `Point<positive decimal>` records containing exactly one fixed pair.
pub fn feature_sketch_fixed_points(
    records: &[FeatureSketchPayloadNamedRecord],
    names: &[FeatureSketchPayloadName],
    fixed_pairs: &[FeatureSketchPayloadFixedPair],
) -> Vec<FeatureSketchFixedPoint> {
    let names = names
        .iter()
        .map(|name| (name.id.as_str(), name))
        .collect::<BTreeMap<_, _>>();
    let fixed_pairs = fixed_pairs
        .iter()
        .map(|pair| (pair.id.as_str(), pair))
        .collect::<BTreeMap<_, _>>();
    records
        .iter()
        .filter_map(|record| {
            if !record.scalar_fields.is_empty() {
                return None;
            }
            let [fixed_pair_id] = record.fixed_pairs.as_slice() else {
                return None;
            };
            let name = names.get(record.name_field.as_str())?;
            parse_sketch_point_name(&name.value)?;
            let pair = fixed_pairs.get(fixed_pair_id.as_str())?;
            Some(FeatureSketchFixedPoint {
                id: record
                    .id
                    .replacen("sketch-payload-record", "sketch-fixed-point", 1),
                operation_label: record.operation_label.clone(),
                named_record: record.id.clone(),
                name: name.value.clone(),
                fixed_pair: pair.id.clone(),
                values: pair.values,
                source_offset: pair.source_offset,
            })
        })
        .collect()
}

/// Group every bit-identical same-name sketch-point witness.
pub fn feature_sketch_point_groups(points: &[FeatureSketchPoint]) -> Vec<FeatureSketchPointGroup> {
    let mut grouped = BTreeSet::new();
    let mut groups = Vec::new();
    for point in points {
        let key = (point.operation_label.as_str(), point.name.as_str());
        if !grouped.insert(key) {
            continue;
        }
        let witnesses = points
            .iter()
            .filter(|candidate| {
                candidate.operation_label == point.operation_label && candidate.name == point.name
            })
            .collect::<Vec<_>>();
        if witnesses.iter().any(|candidate| {
            candidate
                .coordinates
                .iter()
                .zip(point.coordinates)
                .any(|(first, second)| first.to_bits() != second.to_bits())
        }) {
            continue;
        }
        groups.push(FeatureSketchPointGroup {
            id: format!(
                "nx:feature-history:sketch-point-group#{}",
                point.id.rsplit_once('#').map_or("unknown", |(_, key)| key)
            ),
            operation_label: point.operation_label.clone(),
            name: point.name.clone(),
            points: witnesses
                .into_iter()
                .map(|point| point.id.clone())
                .collect(),
            coordinates: point.coordinates,
        });
    }
    groups
}

/// Decode exact named point objects across consecutive offset-store blocks.
pub fn offset_store_named_points(container: &Container) -> Vec<OffsetStoreNamedPoint> {
    let mut points = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        if section
            .records
            .first()
            .is_none_or(|record| record.object_id.is_some())
        {
            continue;
        }
        let section_key = format!("nx:om-data-blocks-{section_ordinal}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for ordinal in 0..section.records.len() {
            let remaining = section.records[ordinal..]
                .iter()
                .map(|record| record.bytes)
                .collect::<Vec<_>>();
            let Some(point) = crate::om::offset_store_named_point(&remaining) else {
                continue;
            };
            let records = &section.records[ordinal..ordinal + point.block_count];
            let first_source = entry_offset + records[0].offset as u64;
            let value_source_offset = |payload_offset: usize| {
                let mut relative = payload_offset;
                for record in records {
                    if relative < record.bytes.len() {
                        return Some(entry_offset + record.offset as u64 + relative as u64);
                    }
                    relative -= record.bytes.len();
                }
                None
            };
            points.push(OffsetStoreNamedPoint {
                id: format!(
                    "nx:offset-store:named-point#{section_ordinal}-{}",
                    ordinal + 1
                ),
                name: point.name,
                data_blocks: (0..point.block_count)
                    .map(|relative| format!("{section_key}:block#{}", ordinal + relative + 1))
                    .collect(),
                values: point.values,
                value_source_offsets: [
                    value_source_offset(point.value_offsets[0]).expect("first scalar in span"),
                    value_source_offset(point.value_offsets[1]).expect("second scalar in span"),
                ],
                source_offset: first_source,
            });
        }
    }
    points
}

/// Join sketch references to named points through exact shared block identity.
pub fn feature_sketch_named_point_block_uses(
    references: &[FeatureSketchReference],
    points: &[OffsetStoreNamedPoint],
) -> Vec<FeatureSketchNamedPointBlockUse> {
    let mut uses = Vec::new();
    for reference in references {
        let Some(data_block) = reference.data_block.as_deref() else {
            continue;
        };
        for point in points {
            let Some(point_block_ordinal) = point
                .data_blocks
                .iter()
                .position(|block| block == data_block)
            else {
                continue;
            };
            let operation_key = reference
                .operation_label
                .rsplit_once('#')
                .map_or(reference.operation_label.as_str(), |(_, key)| key);
            let point_key = point
                .id
                .rsplit_once('#')
                .map_or(point.id.as_str(), |(_, key)| key);
            uses.push(FeatureSketchNamedPointBlockUse {
                id: format!(
                    "nx:feature-history:sketch-named-point-block-use#{operation_key}-{}-{point_key}-{point_block_ordinal}",
                    reference.ordinal
                ),
                operation_label: reference.operation_label.clone(),
                sketch_reference: reference.id.clone(),
                reference_ordinal: reference.ordinal,
                named_point: point.id.clone(),
                data_block: data_block.to_string(),
                point_block_ordinal: point_block_ordinal as u32,
                source_offset: reference.source_offset,
            });
        }
    }
    uses
}

/// Join one named point to a complete sketch lane through unique consecutive block adjacency.
pub fn feature_sketch_preceding_named_point_uses(
    references: &[FeatureSketchReference],
    points: &[OffsetStoreNamedPoint],
) -> Vec<FeatureSketchPrecedingNamedPointUse> {
    fn block_key(block: &str) -> Option<(&str, u32)> {
        let (store, ordinal) = block.rsplit_once(":block#")?;
        Some((store, ordinal.parse().ok()?))
    }

    let mut references_by_operation = BTreeMap::<&str, Vec<&FeatureSketchReference>>::new();
    for reference in references {
        references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut uses = Vec::new();
    for (operation_label, mut operation_references) in references_by_operation {
        operation_references.sort_by_key(|reference| reference.ordinal);
        let complete_lane = !operation_references.is_empty()
            && operation_references
                .iter()
                .enumerate()
                .all(|(ordinal, reference)| {
                    reference.ordinal == ordinal as u32
                        && usize::from(reference.declared_count) == operation_references.len()
                        && reference.data_block.is_some()
                        && reference.terminal == (ordinal + 1 == operation_references.len())
                });
        if !complete_lane {
            continue;
        }
        let first_reference = operation_references[0];
        let first_block = first_reference
            .data_block
            .as_deref()
            .expect("complete lane has resolved references");
        let Some((first_store, first_ordinal)) = block_key(first_block) else {
            continue;
        };
        let candidates = points
            .iter()
            .filter(|point| {
                let Some(last_block) = point.data_blocks.last() else {
                    return false;
                };
                let Some((point_store, point_ordinal)) = block_key(last_block) else {
                    return false;
                };
                point_store == first_store && point_ordinal.checked_add(1) == Some(first_ordinal)
            })
            .collect::<Vec<_>>();
        let [point] = candidates.as_slice() else {
            continue;
        };
        let operation_key = operation_label
            .rsplit_once('#')
            .map_or(operation_label, |(_, key)| key);
        let point_key = point
            .id
            .rsplit_once('#')
            .map_or(point.id.as_str(), |(_, key)| key);
        uses.push(FeatureSketchPrecedingNamedPointUse {
            id: format!(
                "nx:feature-history:sketch-preceding-named-point-use#{operation_key}-{point_key}"
            ),
            operation_label: operation_label.to_string(),
            first_sketch_reference: first_reference.id.clone(),
            named_point: point.id.clone(),
            point_data_blocks: point.data_blocks.clone(),
            following_data_block: first_block.to_string(),
            source_offset: first_reference.source_offset,
        });
    }
    uses
}

/// Join the two exact encodings of a solved sketch point.
pub fn feature_sketch_point_uses(
    point_groups: &[FeatureSketchPointGroup],
    named_points: &[OffsetStoreNamedPoint],
    block_uses: &[FeatureSketchNamedPointBlockUse],
) -> Vec<FeatureSketchPointUse> {
    let named_points = named_points
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    let mut uses = Vec::new();
    let mut joined = BTreeSet::new();
    for block_use in block_uses {
        let key = (
            block_use.operation_label.as_str(),
            block_use.named_point.as_str(),
        );
        if !joined.insert(key) {
            continue;
        }
        let Some(named_point) = named_points.get(block_use.named_point.as_str()) else {
            continue;
        };
        let mut point_block_uses = block_uses
            .iter()
            .filter(|candidate| {
                candidate.operation_label == block_use.operation_label
                    && candidate.named_point == block_use.named_point
            })
            .collect::<Vec<_>>();
        point_block_uses.sort_by_key(|block_use| {
            (
                block_use.reference_ordinal,
                block_use.source_offset,
                block_use.id.as_str(),
            )
        });
        let candidates = point_groups
            .iter()
            .filter(|group| {
                group.operation_label == block_use.operation_label && group.name == named_point.name
            })
            .collect::<Vec<_>>();
        let [point_group] = candidates.as_slice() else {
            continue;
        };
        if point_group
            .coordinates
            .iter()
            .zip(named_point.values)
            .any(|(first, second)| first.to_bits() != second.to_bits())
        {
            continue;
        }
        uses.push(FeatureSketchPointUse {
            id: point_block_uses[0].id.replacen(
                "sketch-named-point-block-use",
                "sketch-point-use",
                1,
            ),
            operation_label: block_use.operation_label.clone(),
            sketch_references: point_block_uses
                .iter()
                .map(|block_use| block_use.sketch_reference.clone())
                .collect(),
            block_uses: point_block_uses
                .iter()
                .map(|block_use| block_use.id.clone())
                .collect(),
            sketch_point_group: point_group.id.clone(),
            named_point: named_point.id.clone(),
            source_offsets: point_block_uses
                .iter()
                .map(|block_use| block_use.source_offset)
                .collect(),
        });
    }
    uses
}

/// Join one uniquely sketch-owned named-point block to a later datum-CSYS construction.
pub fn feature_sketch_datum_csys_dependencies(
    labels: &[FeatureOperationLabel],
    named_points: &[OffsetStoreNamedPoint],
    point_uses: &[FeatureSketchPointUse],
    constructions: &[FeatureDatumCsysConstruction],
) -> Vec<FeatureSketchDatumCsysDependency> {
    fn block_key(block: &str) -> Option<(&str, u32)> {
        let (store, ordinal) = block.rsplit_once(":block#")?;
        Some((store, ordinal.parse().ok()?))
    }

    let positions = labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let points = named_points
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    let mut dependencies = Vec::new();
    for construction in constructions {
        let Some(consumer_position) = positions.get(construction.operation_label.as_str()) else {
            continue;
        };
        let mut candidates = Vec::new();
        for point_use in point_uses {
            let Some(producer_position) = positions.get(point_use.operation_label.as_str()) else {
                continue;
            };
            if producer_position >= consumer_position {
                continue;
            }
            let Some(point) = points.get(point_use.named_point.as_str()) else {
                continue;
            };
            for shared_block in construction
                .data_blocks
                .iter()
                .filter(|block| point.data_blocks.contains(block))
            {
                candidates.push((
                    point_use,
                    FeatureSketchDatumCsysBlockRelation::Shared {
                        data_block: shared_block.clone(),
                    },
                ));
            }
            let Some(point_last_block) = point.data_blocks.last() else {
                continue;
            };
            let Some(construction_first_block) = construction.data_blocks.first() else {
                continue;
            };
            if let (
                Some((point_store, point_ordinal)),
                Some((construction_store, construction_ordinal)),
            ) = (
                block_key(point_last_block),
                block_key(construction_first_block),
            ) {
                if point_store == construction_store
                    && point_ordinal.checked_add(1) == Some(construction_ordinal)
                {
                    candidates.push((
                        point_use,
                        FeatureSketchDatumCsysBlockRelation::Consecutive {
                            point_data_block: point_last_block.clone(),
                            construction_data_block: construction_first_block.clone(),
                        },
                    ));
                }
            }
        }
        candidates.sort_by(|(left_use, left_relation), (right_use, right_relation)| {
            (left_use.id.as_str(), left_relation).cmp(&(right_use.id.as_str(), right_relation))
        });
        candidates.dedup_by(|(left_use, left_relation), (right_use, right_relation)| {
            left_use.id == right_use.id && left_relation == right_relation
        });
        let [(point_use, block_relation)] = candidates.as_slice() else {
            continue;
        };
        dependencies.push(FeatureSketchDatumCsysDependency {
            id: construction.id.replacen(
                "datum-csys-construction",
                "sketch-datum-csys-dependency",
                1,
            ),
            sketch_operation_label: point_use.operation_label.clone(),
            datum_csys_operation_label: construction.operation_label.clone(),
            sketch_point_use: point_use.id.clone(),
            datum_csys_construction: construction.id.clone(),
            block_relation: block_relation.clone(),
            source_offset: point_use.source_offsets[0],
        });
    }
    dependencies.sort_by(|left, right| left.id.cmp(&right.id));
    dependencies
}

pub(crate) fn parse_sketch_point_name(value: &str) -> Option<u32> {
    let suffix = value.strip_prefix("Point")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let ordinal = suffix.parse::<u32>().ok()?;
    (ordinal != 0).then_some(ordinal)
}

pub(crate) type JoinedDataBlockBytes = (Vec<u8>, Vec<u64>, Vec<u64>, Vec<u64>);

pub(crate) fn join_data_block_bytes(
    ids: &[String],
    blocks: &BTreeMap<String, (&[u8], u64)>,
) -> Option<JoinedDataBlockBytes> {
    let source_blocks = ids
        .iter()
        .map(|id| blocks.get(id).copied())
        .collect::<Option<Vec<_>>>()?;
    let byte_len = source_blocks
        .iter()
        .map(|(bytes, _)| bytes.len())
        .sum::<usize>();
    let mut payload = Vec::with_capacity(byte_len);
    let mut block_payload_offsets = Vec::with_capacity(source_blocks.len());
    let mut block_byte_lengths = Vec::with_capacity(source_blocks.len());
    let mut block_source_offsets = Vec::with_capacity(source_blocks.len());
    for (bytes, source_offset) in source_blocks {
        block_payload_offsets.push(payload.len() as u64);
        block_byte_lengths.push(bytes.len() as u64);
        block_source_offsets.push(source_offset);
        payload.extend_from_slice(bytes);
    }
    Some((
        payload,
        block_payload_offsets,
        block_byte_lengths,
        block_source_offsets,
    ))
}

/// Decode and resolve the ordered counted-reference field in sketch payloads.
pub fn feature_sketch_references(container: &Container) -> Vec<FeatureSketchReference> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut references = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(decoded) = crate::om::sketch_payload_references(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let declared_count = decoded.declared_count;
            let terminal_ordinal = decoded.references.len() - 1;
            references.extend(decoded.references.into_iter().enumerate().map(|(ordinal, reference)| {
                let data_block = unique_offset_data_block(&indexed, reference.object_index);
                FeatureSketchReference {
                    id: format!(
                        "nx:feature-history:sketch-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    declared_count,
                    terminal: ordinal == terminal_ordinal,
                    object_index: reference.object_index,
                    data_block,
                    source_offset: entry_offset + reference.offset as u64,
                }
            }));
        }
    }
    references
}

struct ResolvedFeaturePayloadReference {
    section_key: String,
    operation_ordinal: usize,
    ordinal: usize,
    object_index: u32,
    data_block: Option<String>,
    source_offset: u64,
}

fn resolved_feature_payload_references(
    container: &Container,
    decode: impl Fn(crate::om::OperationRecord<'_>) -> Option<Vec<crate::om::PayloadObjectReference>>,
) -> Vec<ResolvedFeaturePayloadReference> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut references = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(decoded) = decode(record) else {
                continue;
            };
            references.extend(decoded.into_iter().enumerate().map(|(ordinal, reference)| {
                ResolvedFeaturePayloadReference {
                    section_key: section_key.clone(),
                    operation_ordinal,
                    ordinal,
                    object_index: reference.object_index,
                    data_block: unique_offset_data_block(&indexed, reference.object_index),
                    source_offset: entry_offset + reference.offset as u64,
                }
            }));
        }
    }
    references
}

/// Decode and resolve the exact ordered construction-reference field in
/// projected-curve payloads without assigning semantic roles to its slots.
pub fn feature_projected_curve_references(
    container: &Container,
) -> Vec<FeatureProjectedCurveReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::projected_curve_payload_references(record).map(|field| field.references)
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeatureProjectedCurveReference {
            id: format!(
                "nx:feature-history:projected-curve-reference#{}-{:010}-{:010}",
                reference.section_key, reference.operation_ordinal, reference.ordinal
            ),
            operation_label,
            ordinal: reference.ordinal as u32,
            object_index: reference.object_index,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        }
    })
    .collect()
}

/// Decode and resolve exact ordered construction references in pattern
/// payloads without assigning seed or transform semantics to their slots.
pub fn feature_pattern_references(container: &Container) -> Vec<FeaturePatternReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::pattern_payload_references(record).map(|field| field.references)
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeaturePatternReference {
            id: format!(
                "nx:feature-history:pattern-reference#{}-{:010}-{:010}",
                reference.section_key, reference.operation_ordinal, reference.ordinal
            ),
            operation_label,
            ordinal: reference.ordinal as u32,
            object_index: reference.object_index,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        }
    })
    .collect()
}

/// Decode exact point-feature construction headers without assigning coordinate semantics.
pub fn feature_point_construction_headers(
    container: &Container,
) -> Vec<FeaturePointConstructionHeader> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut headers = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(header) = crate::om::point_feature_payload_header(record) else {
                continue;
            };
            headers.push(FeaturePointConstructionHeader {
                id: format!(
                    "nx:feature-history:point-construction-header#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                object_index: header.reference.object_index,
                data_block: unique_offset_data_block(&indexed, header.reference.object_index),
                mode: header.mode,
                source_offset: entry_offset + header.reference.offset as u64,
            });
        }
    }
    headers
}

/// Decode exact scalar lanes selected by uniquely resolved point-feature headers.
pub fn feature_point_construction_scalar_lanes(
    container: &Container,
    headers: &[FeaturePointConstructionHeader],
) -> Vec<FeaturePointConstructionScalarLane> {
    let indexed = container.indexed_om_sections();
    let mut lanes = Vec::new();
    for header in headers {
        let Some(expected_target) = header.data_block.as_deref() else {
            continue;
        };
        let Ok(target_ordinal) = usize::try_from(header.object_index) else {
            continue;
        };
        let candidates = indexed
            .iter()
            .enumerate()
            .filter_map(|(section_ordinal, (entry, section))| {
                if section.records.first()?.object_id.is_some() || target_ordinal < 2 {
                    return None;
                }
                let target_id =
                    format!("nx:om-data-blocks-{section_ordinal}:block#{target_ordinal}");
                if target_id != expected_target {
                    return None;
                }
                let preceding = section.records.get(target_ordinal - 2)?;
                let target = section.records.get(target_ordinal - 1)?;
                let lane = crate::om::point_feature_scalar_lane(preceding.bytes, target.bytes)?;
                Some((section_ordinal, *entry, preceding, target, lane))
            })
            .collect::<Vec<_>>();
        let [(section_ordinal, entry, preceding, target, lane)] = candidates.as_slice() else {
            continue;
        };
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let source_offsets = lane.value_offsets.map(|offset| {
            if offset < preceding.bytes.len() {
                entry_offset + preceding.offset as u64 + offset as u64
            } else {
                entry_offset + target.offset as u64 + (offset - preceding.bytes.len()) as u64
            }
        });
        lanes.push(FeaturePointConstructionScalarLane {
            id: header.id.replacen(
                "point-construction-header#",
                "point-construction-scalar-lane#",
                1,
            ),
            operation_label: header.operation_label.clone(),
            construction_header: header.id.clone(),
            data_blocks: [
                format!(
                    "nx:om-data-blocks-{section_ordinal}:block#{}",
                    target_ordinal - 1
                ),
                format!("nx:om-data-blocks-{section_ordinal}:block#{target_ordinal}"),
            ],
            values: lane.values,
            source_offsets,
        });
    }
    lanes
}

/// Decode exact ordered draft construction references without assigning semantic roles.
pub fn feature_draft_construction_references(
    container: &Container,
) -> Vec<FeatureDraftConstructionReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::draft_feature_payload_references(record)
            .map(|field| field.references.into_iter().collect())
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeatureDraftConstructionReference {
            id: format!(
                "nx:feature-history:draft-construction-reference#{}-{:010}-{:010}",
                reference.section_key, reference.operation_ordinal, reference.ordinal
            ),
            operation_label,
            ordinal: reference.ordinal as u32,
            object_index: reference.object_index,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        }
    })
    .collect()
}

/// Decode exact counted compact-index lanes preceding draft construction graphs.
pub fn feature_draft_construction_index_lanes(
    container: &Container,
) -> Vec<FeatureDraftConstructionIndexLane> {
    let indexed = container.indexed_om_sections();
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::draft_feature_leading_index_lane(record) else {
                return;
            };
            let indices = lane
                .indices
                .iter()
                .map(|(value, _)| *value)
                .collect::<Vec<_>>();
            let data_blocks =
                crate::om::draft_feature_payload_references(record).and_then(|graph| {
                    let mut complete_indices = graph
                        .references
                        .iter()
                        .map(|reference| reference.object_index)
                        .collect::<Vec<_>>();
                    complete_indices.extend(&indices);
                    let section_ordinal = unique_offset_data_store(&indexed, &complete_indices)?;
                    Some(
                        indices
                            .iter()
                            .map(|index| {
                                format!("nx:om-data-blocks-{section_ordinal}:block#{index}")
                            })
                            .collect(),
                    )
                });
            lanes.push(FeatureDraftConstructionIndexLane {
                id: format!(
                    "nx:feature-history:draft-construction-index-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                declared_count: lane.declared_count,
                indices,
                data_blocks,
                source_offsets: lane
                    .indices
                    .iter()
                    .map(|(_, offset)| entry_offset + *offset as u64)
                    .collect(),
            });
        },
    );
    lanes
}

/// Reconstruct ordered logical payloads from resolved draft index lanes.
pub fn feature_draft_construction_payloads(
    container: &Container,
    lanes: &[FeatureDraftConstructionIndexLane],
) -> Vec<FeatureDraftConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    lanes
        .iter()
        .filter_map(|lane| {
            let data_blocks = lane.data_blocks.clone()?;
            let (bytes, block_payload_offsets, block_byte_lengths, block_source_offsets) =
                join_data_block_bytes(&data_blocks, &blocks)?;
            Some(FeatureDraftConstructionPayload {
                id: lane.id.replacen(
                    "draft-construction-index-lane#",
                    "draft-construction-payload#",
                    1,
                ),
                operation_label: lane.operation_label.clone(),
                index_lane: lane.id.clone(),
                data_blocks,
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&bytes),
                block_payload_offsets,
                block_byte_lengths,
                block_source_offsets,
            })
        })
        .collect()
}

/// Decode complete end-anchored terminal lanes from draft construction payloads.
pub fn feature_draft_construction_terminal_lanes(
    container: &Container,
) -> Vec<FeatureDraftConstructionTerminalLane> {
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::draft_feature_terminal_lane(record) else {
                return;
            };
            lanes.push(FeatureDraftConstructionTerminalLane {
                id: format!(
                    "nx:feature-history:draft-construction-terminal-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                indices: lane.indices,
                tail: lane.tail,
                index_source_offsets: lane
                    .index_offsets
                    .map(|offset| entry_offset + offset as u64),
                source_offset: entry_offset + lane.offset as u64,
            });
        },
    );
    lanes
}

/// Decode and resolve the exact common reference envelope in surface-feature
/// payloads without assigning section or guide semantics to its slots.
pub fn feature_surface_construction_references(
    container: &Container,
) -> Vec<FeatureSurfaceConstructionReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::surface_feature_payload_references(record)
            .map(|field| field.references.into_iter().collect())
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeatureSurfaceConstructionReference {
            id: format!(
                "nx:feature-history:surface-construction-reference#{}-{:010}-{:010}",
                reference.section_key, reference.operation_ordinal, reference.ordinal
            ),
            operation_label,
            ordinal: reference.ordinal as u32,
            object_index: reference.object_index,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        }
    })
    .collect()
}

/// Decode and resolve exact counted surface-construction branches without
/// assigning section or guide semantics to their members.
pub fn feature_surface_construction_branches(
    container: &Container,
) -> Vec<FeatureSurfaceConstructionBranch> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut branches = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(group) = crate::om::surface_feature_payload_branches(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            branches.extend(group.branches.into_iter().enumerate().map(|(ordinal, branch)| {
                let resolve = |ordinal: usize, reference: crate::om::PayloadObjectReference| {
                    FeatureSurfaceBranchReference {
                        ordinal: ordinal as u32,
                        object_index: reference.object_index,
                        data_block: unique_offset_data_block(&indexed, reference.object_index),
                        source_offset: entry_offset + reference.offset as u64,
                    }
                };
                let members = branch
                    .members
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, reference)| resolve(ordinal, reference))
                    .collect::<Vec<_>>();
                let terminal = resolve(members.len(), branch.terminal);
                FeatureSurfaceConstructionBranch {
                    id: format!(
                        "nx:feature-history:surface-construction-branch#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    family: group.family,
                    header_code: group.header_code,
                    mode: branch.mode,
                    declared_count: branch.declared_count,
                    witnessed: branch.witnessed,
                    members,
                    terminal,
                    suffix: branch.suffix,
                    source_offset: entry_offset + branch.offset as u64,
                }
            }));
        }
    }
    branches
}

/// Decode and resolve the witnessed ordered profile list in extrusion payloads.
pub fn feature_extrude_profile_references(
    container: &Container,
) -> Vec<FeatureExtrudeProfileReference> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut references = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(decoded) = crate::om::extrude_profile_references(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let witnessed = decoded.witnessed;
            references.extend(decoded.references.into_iter().enumerate().map(|(ordinal, reference)| {
                FeatureExtrudeProfileReference {
                    id: format!(
                        "nx:feature-history:extrude-profile-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    witnessed,
                    object_index: reference.object_index,
                    data_block: unique_offset_data_block(&indexed, reference.object_index),
                    source_offset: entry_offset + reference.offset as u64,
                }
            }));
        }
    }
    references
}

/// Decode fixed scalar headers from bounded extrusion payloads.
pub fn feature_extrude_payload_headers(container: &Container) -> Vec<FeatureExtrudePayloadHeader> {
    let sections = container.om_sections();
    let mut headers = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(header) = crate::om::extrude_payload_header(record) else {
                continue;
            };
            headers.push(FeatureExtrudePayloadHeader {
                id: format!(
                    "nx:feature-history:extrude-payload-header#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                scalars: header.scalars,
                source_offset: entry_offset + header.offset as u64,
            });
        }
    }
    headers
}

/// Decode exact terminal discriminator lanes from bounded extrusion payloads.
pub fn feature_extrude_payload_footers(container: &Container) -> Vec<FeatureExtrudePayloadFooter> {
    let sections = container.om_sections();
    let mut footers = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(footer) = crate::om::extrude_payload_footer(record) else {
                continue;
            };
            footers.push(FeatureExtrudePayloadFooter {
                id: format!(
                    "nx:feature-history:extrude-payload-footer#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                type_indices: footer.type_indices,
                mode_indices: footer.mode_indices,
                flags: footer.flags,
                trailing_indices: footer.trailing_indices,
                source_offset: entry_offset + footer.offset as u64,
            });
        }
    }
    footers
}

/// Decode typed scalar clauses anchored to operation body-reference fields.
pub fn feature_operation_body_scalar_triples(
    container: &Container,
) -> Vec<FeatureOperationBodyScalarTriple> {
    let sections = container.om_sections();
    let mut triples = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            for triple in crate::om::operation_body_scalar_triples(record) {
                let encoding = |encoding| match encoding {
                    crate::om::PayloadScalarEncoding::Zero => FeaturePayloadScalarEncoding::Zero,
                    crate::om::PayloadScalarEncoding::Binary32 => {
                        FeaturePayloadScalarEncoding::Binary32
                    }
                    crate::om::PayloadScalarEncoding::Binary64 => {
                        FeaturePayloadScalarEncoding::Binary64
                    }
                };
                triples.push(FeatureOperationBodyScalarTriple {
                    id: format!(
                        "nx:feature-history:operation-body-scalar-triple#{section_key}-{operation_ordinal:010}-{}",
                        triple.body_reference_ordinal
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                    ),
                    body_reference_ordinal: triple.body_reference_ordinal,
                    body_object_index: triple.body_object_index,
                    branch: triple.branch,
                    values: triple.scalars.map(|scalar| scalar.value),
                    encodings: triple.scalars.map(|scalar| encoding(scalar.encoding)),
                    source_offsets: triple
                        .scalars
                        .map(|scalar| entry_offset + scalar.offset as u64),
                });
            }
        }
    }
    triples
}

/// Decode ordered member lanes following branch-`11` operation body clauses.
pub fn feature_operation_body_members(container: &Container) -> Vec<FeatureOperationBodyMember> {
    let sections = container.om_sections();
    let mut members = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            members.extend(
                crate::om::operation_body_members(record)
                    .into_iter()
                    .map(|member| FeatureOperationBodyMember {
                        id: format!(
                            "nx:feature-history:operation-body-member#{section_key}-{operation_ordinal:010}-{}-{}",
                            member.body_reference_ordinal, member.ordinal
                        ),
                        operation_label: format!(
                            "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                        ),
                        body_reference_ordinal: member.body_reference_ordinal,
                        body_object_index: member.body_object_index,
                        ordinal: member.ordinal,
                        member_index: member.member_index,
                        source_offset: entry_offset + member.offset as u64,
                    }),
            );
        }
    }
    members
}

/// Resolve wrapped operation members that name known feature-body identities.
pub fn feature_operation_body_operands(
    members: &[FeatureOperationBodyMember],
    references: &[FeatureBodyReferenceOccurrence],
    bindings: &[SegmentBodyBinding],
) -> Vec<FeatureOperationBodyOperand> {
    let known = references
        .iter()
        .map(|reference| reference.body_object_index)
        .chain(
            bindings
                .iter()
                .flat_map(|binding| [binding.body_object_index, binding.body_alias_object_index]),
        )
        .collect::<BTreeSet<_>>();
    members
        .iter()
        .filter(|member| {
            member.member_index != member.body_object_index && known.contains(&member.member_index)
        })
        .map(|member| FeatureOperationBodyOperand {
            id: member
                .id
                .replacen("operation-body-member", "operation-body-operand", 1),
            operation_label: member.operation_label.clone(),
            body_object_index: member.body_object_index,
            body_reference_ordinal: member.body_reference_ordinal,
            ordinal: member.ordinal,
            operand_object_index: member.member_index,
            segment_body_bindings: bindings
                .iter()
                .filter(|binding| {
                    binding.body_object_index == member.member_index
                        || binding.body_alias_object_index == member.member_index
                })
                .map(|binding| binding.id.clone())
                .collect(),
            source_offset: member.source_offset,
        })
        .collect()
}

/// Decode exact continuations following `TRIM BODY` branch-`11` member lanes.
pub fn feature_operation_body_11_continuations(
    container: &Container,
) -> Vec<FeatureOperationBody11Continuation> {
    let sections = container.om_sections();
    let mut continuations = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            continuations.extend(
                crate::om::operation_body_11_continuations(record)
                    .into_iter()
                    .map(|continuation| FeatureOperationBody11Continuation {
                        id: format!(
                            "nx:feature-history:trim-body-11-continuation#{section_key}-{operation_ordinal:010}-{}",
                            continuation.body_reference_ordinal
                        ),
                        operation_label: format!(
                            "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                        ),
                        body_reference_ordinal: continuation.body_reference_ordinal,
                        body_object_index: continuation.body_object_index,
                        continuation_index: continuation.continuation_index,
                        continuation_source_offset: entry_offset
                            + continuation.continuation_offset as u64,
                        terminal_object_index: continuation.terminal_object_index,
                        terminal_source_offset: entry_offset + continuation.terminal_offset as u64,
                    }),
            );
        }
    }
    continuations
}

/// Decode complete unwrapped counted reference lanes following body scalar clauses.
pub fn feature_operation_body_reference_lanes(
    container: &Container,
) -> Vec<FeatureOperationBodyReferenceLane> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut lanes = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            for lane in crate::om::operation_body_reference_lanes(record) {
                let encoding = match lane.encoding {
                    crate::om::OperationBodyReferenceLaneEncoding::CompactIndex => {
                        FeatureOperationBodyReferenceLaneEncoding::CompactIndex
                    }
                    crate::om::OperationBodyReferenceLaneEncoding::PayloadObjectIndex => {
                        FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex
                    }
                };
                let object_indices = lane
                    .values
                    .iter()
                    .map(|value| value.object_index)
                    .collect::<Vec<_>>();
                let data_blocks = object_indices
                    .iter()
                    .map(|index| unique_offset_data_block(&indexed, *index))
                    .collect();
                lanes.push(FeatureOperationBodyReferenceLane {
                    id: format!(
                        "nx:feature-history:operation-body-reference-lane#{section_key}-{operation_ordinal:010}-{}",
                        lane.body_reference_ordinal
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                    ),
                    body_reference_ordinal: lane.body_reference_ordinal,
                    body_object_index: lane.body_object_index,
                    branch: lane.branch,
                    encoding,
                    object_indices,
                    data_blocks,
                    source_offsets: lane
                        .values
                        .iter()
                        .map(|value| entry_offset + value.offset as u64)
                        .collect(),
                });
            }
        }
    }
    lanes
}

/// Join the two exact encodings of an extrusion construction profile.
pub fn feature_extrude_construction_profiles(
    references: &[FeatureExtrudeProfileReference],
    lanes: &[FeatureOperationBodyReferenceLane],
) -> Vec<FeatureExtrudeConstructionProfile> {
    let mut references_by_operation = BTreeMap::<&str, Vec<&FeatureExtrudeProfileReference>>::new();
    for reference in references {
        references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut profiles = Vec::new();
    for (operation_label, mut operation_references) in references_by_operation {
        operation_references.sort_by_key(|reference| reference.ordinal);
        if operation_references.is_empty()
            || operation_references
                .iter()
                .enumerate()
                .any(|(ordinal, reference)| {
                    reference.ordinal != ordinal as u32 || !reference.witnessed
                })
        {
            continue;
        }
        let object_indices = operation_references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>();
        let matching_lanes = lanes
            .iter()
            .filter(|lane| {
                lane.operation_label == operation_label
                    && lane.branch == 0x11
                    && lane.encoding
                        == FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex
                    && lane.object_indices == object_indices
            })
            .collect::<Vec<_>>();
        let [lane] = matching_lanes.as_slice() else {
            continue;
        };
        let Some(data_blocks) = operation_references
            .iter()
            .map(|reference| reference.data_block.clone())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(lane_data_blocks) = lane.data_blocks.iter().cloned().collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if lane_data_blocks != data_blocks {
            continue;
        }
        profiles.push(FeatureExtrudeConstructionProfile {
            id: operation_label.replacen("operation-label", "extrude-construction-profile", 1),
            operation_label: operation_label.to_string(),
            body_object_index: lane.body_object_index,
            object_indices,
            data_blocks,
            profile_source_offsets: operation_references
                .iter()
                .map(|reference| reference.source_offset)
                .collect(),
            body_lane_source_offsets: lane.source_offsets.clone(),
        });
    }
    profiles
}

/// Decode structured `32` branches following extrusion body-reference fields.
pub fn feature_extrude_payload_32_branches(
    container: &Container,
) -> Vec<FeatureExtrudePayload32Branch> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut branches = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(branch) = crate::om::extrude_payload_32_branch(record) else {
                continue;
            };
            let resolve = |indices: &[u32]| {
                indices
                    .iter()
                    .map(|index| unique_offset_data_block(&indexed, *index))
                    .collect::<Vec<_>>()
            };
            let atom_data_blocks = resolve(&branch.atom_indices);
            let first_data_blocks = resolve(&branch.first_indices);
            let second_data_blocks = resolve(&branch.second_indices);
            branches.push(FeatureExtrudePayload32Branch {
                id: format!(
                    "nx:feature-history:extrude-payload-32-branch#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                body_object_index: branch.body_object_index,
                scalar: branch.scalar,
                atoms_be: branch.atoms_be,
                atom_indices: branch.atom_indices,
                atom_data_blocks,
                first_indices: branch.first_indices,
                first_data_blocks,
                second_indices: branch.second_indices,
                second_data_blocks,
                terminal_object_index: branch.terminal_object_index,
                source_offset: entry_offset + branch.offset as u64,
            });
        }
    }
    branches
}

/// Join exact profile fields to self-witnessed structured extrusion branches.
pub fn feature_extrude_32_constructions(
    references: &[FeatureExtrudeProfileReference],
    branches: &[FeatureExtrudePayload32Branch],
) -> Vec<FeatureExtrude32Construction> {
    let mut branches_by_operation = BTreeMap::<&str, Vec<&FeatureExtrudePayload32Branch>>::new();
    for branch in branches {
        branches_by_operation
            .entry(branch.operation_label.as_str())
            .or_default()
            .push(branch);
    }
    let mut constructions = Vec::new();
    for (operation_label, operation_branches) in branches_by_operation {
        let [branch] = operation_branches.as_slice() else {
            continue;
        };
        let mut profile = references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
            .collect::<Vec<_>>();
        profile.sort_by_key(|reference| reference.ordinal);
        if profile.is_empty()
            || profile
                .iter()
                .enumerate()
                .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
        {
            continue;
        }
        let Some(profile_data_blocks) = profile
            .iter()
            .map(|reference| reference.data_block.clone())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(atom_data_blocks) = branch.atom_data_blocks.iter().cloned().collect() else {
            continue;
        };
        let Some(first_data_blocks) = branch.first_data_blocks.iter().cloned().collect() else {
            continue;
        };
        let Some(second_data_blocks) = branch.second_data_blocks.iter().cloned().collect() else {
            continue;
        };
        constructions.push(FeatureExtrude32Construction {
            id: branch
                .id
                .replacen("extrude-payload-32-branch", "extrude-32-construction", 1),
            operation_label: branch.operation_label.clone(),
            branch: branch.id.clone(),
            body_object_index: branch.body_object_index,
            profile_references: profile
                .iter()
                .map(|reference| reference.id.clone())
                .collect(),
            profile_data_blocks,
            atom_data_blocks,
            first_data_blocks,
            second_data_blocks,
        });
    }
    constructions
}

/// Decode and resolve ordered construction references in `BLOCK` payloads.
pub fn feature_block_construction_references(
    container: &Container,
) -> Vec<FeatureBlockConstructionReference> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut references = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(field) = crate::om::block_construction_references(record) else {
                continue;
            };
            let terminal_ordinal = field.references.len() - 1;
            references.extend(field.references.into_iter().enumerate().map(
                |(ordinal, reference)| FeatureBlockConstructionReference {
                    id: format!(
                        "nx:feature-history:block-construction-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                    ),
                    control: field.control,
                    ordinal: ordinal as u32,
                    terminal: ordinal == terminal_ordinal,
                    object_index: reference.object_index,
                    data_block: unique_offset_data_block(&indexed, reference.object_index),
                    source_offset: entry_offset + reference.offset as u64,
                },
            ));
        }
    }
    references
}

/// Join complete, uniquely resolved `BLOCK` construction-reference fields.
pub fn feature_block_constructions(
    references: &[FeatureBlockConstructionReference],
) -> Vec<FeatureBlockConstruction> {
    let mut by_operation = BTreeMap::<&str, Vec<&FeatureBlockConstructionReference>>::new();
    for reference in references {
        by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut constructions = Vec::new();
    for (operation_label, mut field) in by_operation {
        field.sort_by_key(|reference| reference.ordinal);
        if field.len() != 19
            || field.iter().enumerate().any(|(ordinal, reference)| {
                reference.ordinal != ordinal as u32
                    || reference.control != field[0].control
                    || reference.terminal != (ordinal == 18)
            })
        {
            continue;
        }
        let (terminal, members) = field.split_last().expect("nineteen references");
        let Some(member_data_blocks) = members
            .iter()
            .map(|reference| reference.data_block.clone())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(terminal_data_block) = terminal.data_block.clone() else {
            continue;
        };
        constructions.push(FeatureBlockConstruction {
            id: operation_label.replacen("operation-label", "block-construction", 1),
            operation_label: operation_label.to_string(),
            control: field[0].control,
            member_references: members
                .iter()
                .map(|reference| reference.id.clone())
                .collect(),
            member_data_blocks,
            terminal_reference: terminal.id.clone(),
            terminal_data_block,
        });
    }
    constructions
}

/// Reconstruct complete `BLOCK` construction payloads in reference order.
pub fn feature_block_construction_payloads(
    container: &Container,
    constructions: &[FeatureBlockConstruction],
) -> Vec<FeatureBlockConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .filter_map(|construction| {
            let mut data_blocks = construction.member_data_blocks.clone();
            data_blocks.push(construction.terminal_data_block.clone());
            let (bytes, block_payload_offsets, block_byte_lengths, block_source_offsets) =
                join_data_block_bytes(&data_blocks, &blocks)?;
            Some(FeatureBlockConstructionPayload {
                id: construction
                    .id
                    .replacen("block-construction", "block-construction-payload", 1),
                operation_label: construction.operation_label.clone(),
                construction: construction.id.clone(),
                data_blocks,
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&bytes),
                block_payload_offsets,
                block_byte_lengths,
                block_source_offsets,
            })
        })
        .collect()
}

/// Decode exact framed scalar fields across reconstructed `BLOCK` payloads.
pub fn feature_block_payload_scalars(
    container: &Container,
    payloads: &[FeatureBlockConstructionPayload],
) -> Vec<FeatureBlockPayloadScalar> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::construction_payload_scalar_fields(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, field)| {
                    let source_offset = joined_payload_source_offset(
                        field.offset as u64,
                        &starts,
                        &lengths,
                        &sources,
                    )?;
                    Some(FeatureBlockPayloadScalar {
                        id: format!("{}-scalar-{ordinal}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        field_code: field.field_code,
                        value: field.value,
                        payload_offset: field.offset as u64,
                        source_offset,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode exact compact-code name fields across reconstructed `BLOCK` payloads.
pub fn feature_block_payload_names(
    container: &Container,
    payloads: &[FeatureBlockConstructionPayload],
) -> Vec<FeatureBlockPayloadName> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::construction_payload_named_fields(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, field)| {
                    let source_offset = joined_payload_source_offset(
                        field.offset as u64,
                        &starts,
                        &lengths,
                        &sources,
                    )?;
                    Some(FeatureBlockPayloadName {
                        id: format!("{}-name-{ordinal}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        type_code: field.type_code,
                        payload_leading: field.payload_leading,
                        value: field.value.to_string(),
                        payload_offset: field.offset as u64,
                        source_offset,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join complete `BLOCK` payload names to scalar fields in their intervals.
pub fn feature_block_payload_named_records(
    payloads: &[FeatureBlockConstructionPayload],
    names: &[FeatureBlockPayloadName],
    scalars: &[FeatureBlockPayloadScalar],
) -> Vec<FeatureBlockPayloadNamedRecord> {
    let mut records = Vec::new();
    for payload in payloads {
        let mut payload_names = names
            .iter()
            .filter(|name| name.construction_payload == payload.id)
            .collect::<Vec<_>>();
        payload_names.sort_by_key(|name| name.payload_offset);
        for (ordinal, name) in payload_names.iter().enumerate() {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload.byte_len, |next| next.payload_offset);
            let mut scalar_fields = scalars
                .iter()
                .filter(|scalar| {
                    scalar.construction_payload == payload.id
                        && scalar.payload_offset > name.payload_offset
                        && scalar.payload_offset < end
                })
                .collect::<Vec<_>>();
            scalar_fields.sort_by_key(|scalar| scalar.payload_offset);
            records.push(FeatureBlockPayloadNamedRecord {
                id: format!("{}-record-{ordinal}", payload.id),
                operation_label: payload.operation_label.clone(),
                construction_payload: payload.id.clone(),
                name_field: name.id.clone(),
                scalar_fields: scalar_fields
                    .into_iter()
                    .map(|scalar| scalar.id.clone())
                    .collect(),
                payload_start_offset: name.payload_offset,
                payload_end_offset: end,
            });
        }
    }
    records
}

/// Type exact two-scalar `Point<positive decimal>` `BLOCK` payload intervals.
pub fn feature_block_payload_points(
    records: &[FeatureBlockPayloadNamedRecord],
    names: &[FeatureBlockPayloadName],
    scalars: &[FeatureBlockPayloadScalar],
) -> Vec<FeatureBlockPayloadPoint> {
    let names = names
        .iter()
        .map(|name| (name.id.as_str(), name))
        .collect::<BTreeMap<_, _>>();
    let scalars = scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<BTreeMap<_, _>>();
    records
        .iter()
        .filter_map(|record| {
            let name = names.get(record.name_field.as_str())?;
            parse_sketch_point_name(&name.value)?;
            let [first_id, second_id] = record.scalar_fields.as_slice() else {
                return None;
            };
            let first = scalars.get(first_id.as_str())?;
            let second = scalars.get(second_id.as_str())?;
            Some(FeatureBlockPayloadPoint {
                id: format!("{}-point", record.id),
                operation_label: record.operation_label.clone(),
                named_record: record.id.clone(),
                name: name.value.clone(),
                scalar_fields: [first.id.clone(), second.id.clone()],
                coordinates: [first.value, second.value],
            })
        })
        .collect()
}

/// Group every bit-identical same-name `BLOCK` construction-point witness.
pub fn feature_block_payload_point_groups(
    points: &[FeatureBlockPayloadPoint],
) -> Vec<FeatureBlockPayloadPointGroup> {
    let mut grouped = BTreeSet::new();
    let mut groups = Vec::new();
    for point in points {
        let key = (point.operation_label.as_str(), point.name.as_str());
        if !grouped.insert(key) {
            continue;
        }
        let witnesses = points
            .iter()
            .filter(|candidate| {
                candidate.operation_label == point.operation_label && candidate.name == point.name
            })
            .collect::<Vec<_>>();
        if witnesses.iter().any(|candidate| {
            candidate
                .coordinates
                .iter()
                .zip(point.coordinates)
                .any(|(first, second)| first.to_bits() != second.to_bits())
        }) {
            continue;
        }
        groups.push(FeatureBlockPayloadPointGroup {
            id: format!("{}-group", point.id),
            operation_label: point.operation_label.clone(),
            name: point.name.clone(),
            points: witnesses
                .into_iter()
                .map(|point| point.id.clone())
                .collect(),
            coordinates: point.coordinates,
        });
    }
    groups
}

fn joined_payload_source_offset(
    relative: u64,
    starts: &[u64],
    lengths: &[u64],
    sources: &[u64],
) -> Option<u64> {
    starts
        .iter()
        .zip(lengths)
        .zip(sources)
        .find_map(|((start, length), source)| {
            (relative >= *start && relative < start.saturating_add(*length))
                .then_some(source + relative - start)
        })
}

/// Resolve the consecutive three-parameter dimension run of `BLOCK` features.
pub fn feature_block_dimensions(
    constructions: &[FeatureBlockConstruction],
    bindings: &[FeatureParameterBinding],
    declarations: &[ExpressionDeclaration],
    expressions: &[Expression],
) -> Vec<FeatureBlockDimensions> {
    constructions
        .iter()
        .filter_map(|construction| {
            let mut operation_bindings = bindings
                .iter()
                .filter(|binding| binding.operation_label == construction.operation_label)
                .collect::<Vec<_>>();
            operation_bindings
                .sort_by_key(|binding| (binding.input_slot, binding.reference_ordinal));
            let mut anchors = operation_bindings
                .iter()
                .map(|binding| binding.expression_declaration.as_str())
                .collect::<Vec<_>>();
            anchors.sort_unstable();
            anchors.dedup();
            let [anchor] = anchors.as_slice() else {
                return None;
            };
            let start = declarations
                .iter()
                .position(|declaration| declaration.id == *anchor)?;
            let run: [&ExpressionDeclaration; 3] = declarations
                .get(start..start + 3)?
                .iter()
                .collect::<Vec<_>>()
                .try_into()
                .ok()?;
            let first = run[0].parameter_index;
            if run.iter().enumerate().any(|(ordinal, declaration)| {
                declaration.record.split_once(":entry#").map(|pair| pair.0)
                    != run[0].record.split_once(":entry#").map(|pair| pair.0)
                    || declaration.source_entry != run[0].source_entry
                    || Some(declaration.parameter_index) != first.checked_add(ordinal as u32)
                    || declaration.name != format!("p{}", declaration.parameter_index)
                    || declaration.qualifier.is_some()
            }) {
                return None;
            }
            let resolved: [(&Expression, f64); 3] = run
                .iter()
                .map(|declaration| {
                    let mut matches = expressions.iter().filter(|expression| {
                        expression.declaration.as_deref() == Some(&declaration.id)
                    });
                    let expression = matches.next()?;
                    if matches.next().is_some() || expression.unit != ExpressionUnit::Millimeter {
                        return None;
                    }
                    Some((
                        expression,
                        expression.value.filter(|value| value.is_finite())?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()?;
            if resolved[0].0.source_table.is_empty()
                || resolved
                    .iter()
                    .zip(run)
                    .any(|((expression, _), declaration)| {
                        expression.source_entry != declaration.source_entry
                            || expression.source_table != resolved[0].0.source_table
                    })
            {
                return None;
            }
            Some(FeatureBlockDimensions {
                id: construction
                    .id
                    .replacen("block-construction", "block-dimensions", 1),
                operation_label: construction.operation_label.clone(),
                construction: construction.id.clone(),
                anchor_bindings: operation_bindings
                    .into_iter()
                    .map(|binding| binding.id.clone())
                    .collect(),
                declarations: run.map(|declaration| declaration.id.clone()),
                expressions: resolved
                    .each_ref()
                    .map(|(expression, _)| expression.id.clone()),
                values: resolved.map(|(_, value)| value),
            })
        })
        .collect()
}

/// Decode persistent object frames from bounded offset-store blocks.
pub fn data_block_object_frames(container: &Container) -> Vec<DataBlockObjectFrame> {
    let blocks = offset_data_block_bytes(container);
    blocks
        .into_iter()
        .flat_map(|(data_block, (bytes, source_offset))| {
            crate::om::data_block_object_frames(bytes)
                .into_iter()
                .enumerate()
                .map(|(ordinal, frame)| DataBlockObjectFrame {
                    id: data_block_object_frame_id(&data_block, ordinal),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    object_id: frame.object_id,
                    source_offset: source_offset + frame.offset as u64,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn data_block_object_frame_id(data_block: &str, ordinal: usize) -> String {
    format!(
        "{}-{ordinal}",
        data_block
            .replacen("nx:om-data-blocks-", "nx:om-data-block-object-frames-", 1)
            .replacen(":block#", ":block-frame#", 1)
    )
}

fn unique_offset_data_block(
    indexed: &[(&crate::container::DirEntry, crate::om::IndexedSection<'_>)],
    object_index: u32,
) -> Option<String> {
    let section_ordinal = unique_offset_data_store(indexed, &[object_index])?;
    Some(format!(
        "nx:om-data-blocks-{section_ordinal}:block#{object_index}"
    ))
}

fn unique_offset_data_store(
    indexed: &[(&crate::container::DirEntry, crate::om::IndexedSection<'_>)],
    object_indices: &[u32],
) -> Option<usize> {
    if object_indices.is_empty() || object_indices.contains(&0) {
        return None;
    }
    let candidates = indexed
        .iter()
        .enumerate()
        .filter(|(_, (_, candidate))| {
            candidate
                .records
                .first()
                .is_some_and(|record| record.object_id.is_none())
                && object_indices.iter().all(|object_index| {
                    usize::try_from(*object_index)
                        .ok()
                        .is_some_and(|ordinal| ordinal <= candidate.records.len())
                })
        })
        .map(|(section_ordinal, _)| section_ordinal)
        .collect::<Vec<_>>();
    let [section_ordinal] = candidates.as_slice() else {
        return None;
    };
    Some(*section_ordinal)
}

/// Join operation input lanes to uniquely resolved parameter declarations.
pub fn feature_parameter_bindings(
    inputs: &[FeatureInputBlock],
    references: &[DataBlockReference],
    expressions: &[Expression],
) -> Vec<FeatureParameterBinding> {
    let mut expressions_by_declaration = BTreeMap::<&str, Vec<&str>>::new();
    for expression in expressions {
        if let Some(declaration) = expression.declaration.as_deref() {
            expressions_by_declaration
                .entry(declaration)
                .or_default()
                .push(expression.id.as_str());
        }
    }
    let mut bindings = Vec::new();
    for input in inputs {
        for reference in references
            .iter()
            .filter(|reference| reference.data_block == input.data_block)
        {
            let Some(expression_declaration) = &reference.target_expression_declaration else {
                continue;
            };
            let operation_key = input
                .operation_label
                .rsplit_once('#')
                .map_or(input.operation_label.as_str(), |(_, key)| key);
            bindings.push(FeatureParameterBinding {
                id: format!(
                    "nx:feature-history:parameter-binding#{operation_key}-{}-{}",
                    input.input_slot, reference.ordinal
                ),
                operation_label: input.operation_label.clone(),
                input_slot: input.input_slot,
                input_block: input.data_block.clone(),
                reference_ordinal: reference.ordinal,
                expression_declaration: expression_declaration.clone(),
                expression: expressions_by_declaration
                    .get(expression_declaration.as_str())
                    .and_then(|matches| matches.as_slice().first().filter(|_| matches.len() == 1))
                    .map(|expression| (*expression).to_string()),
                object_id: reference.object_id,
                source_offset: reference.source_offset,
            });
        }
    }
    bindings
}

/// Group exact expression bindings by consuming operation and expression.
pub fn feature_parameter_uses(bindings: &[FeatureParameterBinding]) -> Vec<FeatureParameterUse> {
    let mut grouped = BTreeMap::<(&str, &str), Vec<&FeatureParameterBinding>>::new();
    for binding in bindings {
        if let Some(expression) = binding.expression.as_deref() {
            grouped
                .entry((binding.operation_label.as_str(), expression))
                .or_default()
                .push(binding);
        }
    }
    let mut uses = grouped
        .into_iter()
        .map(|((operation_label, expression), mut bindings)| {
            bindings.sort_by_key(|binding| binding.source_offset);
            (operation_label, expression, bindings)
        })
        .collect::<Vec<_>>();
    uses.sort_by_key(|(_, _, bindings)| bindings[0].source_offset);
    uses.into_iter()
        .map(|(operation_label, expression, bindings)| {
            let operation_key = operation_label
                .rsplit_once('#')
                .map_or(operation_label, |(_, key)| key);
            let expression_key = expression
                .rsplit_once('#')
                .map_or(expression, |(_, key)| key);
            FeatureParameterUse {
                id: format!("nx:feature-history:parameter-use#{operation_key}-{expression_key}"),
                operation_label: operation_label.to_string(),
                expression: expression.to_string(),
                bindings: bindings.iter().map(|binding| binding.id.clone()).collect(),
                source_offsets: bindings
                    .iter()
                    .map(|binding| binding.source_offset)
                    .collect(),
            }
        })
        .collect()
}

/// Resolve segment-index words that point to validated framed OM sections.
pub fn segment_om_links(container: &Container) -> Vec<SegmentOmLink> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    let entry_start = usize::try_from(entry_offset).expect("in-bounds directory offset");
    let sections = container
        .om_sections()
        .into_iter()
        .filter(|(candidate, _)| candidate.name == entry.name)
        .map(|(_, section)| {
            let has = |name| {
                section
                    .types
                    .iter()
                    .any(|definition| definition.name == name)
            };
            let role = if has("UGS::FEATURE_RECORD") {
                OmSchemaRole::FeatureHistory
            } else if has("UGS::EXP_expression") {
                OmSchemaRole::Expressions
            } else if has("UGS::Solid::Topol") {
                OmSchemaRole::Model
            } else if has("UGS::OM::SaveAuditTrail") {
                OmSchemaRole::AuditTrail
            } else {
                OmSchemaRole::Other
            };
            (section.offset, role)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut links = Vec::new();
    for (row_ordinal, row) in index.rows.into_iter().enumerate() {
        for (slot, relative) in [
            (SegmentIndexSlot::TypeCode, row.type_code),
            (SegmentIndexSlot::SubtypeCode, row.subtype_code),
            (SegmentIndexSlot::Value, row.value),
        ] {
            let relative = relative as usize;
            let (separator_byte_len, schema_role) = if let Some(role) = sections.get(&relative) {
                (0usize, *role)
            } else if container
                .data
                .get(entry_start + relative..entry_start + relative + 4)
                == Some(&[0xc0, 0xd1, 0xf1, 0xed])
            {
                let Some(role) = sections.get(&(relative + 4)) else {
                    continue;
                };
                (4, *role)
            } else {
                continue;
            };
            links.push(SegmentOmLink {
                id: format!("nx:segment-om-links:link#{}", links.len()),
                row: format!("nx:segment-index:row#{row_ordinal}"),
                slot,
                schema_role,
                separator_byte_len: separator_byte_len as u32,
                source_offset: entry_offset + relative as u64,
                section_offset: entry_offset + relative as u64 + separator_byte_len as u64,
            });
        }
    }
    links
}

/// Resolve segment-index words that point to validated compressed wrappers.
pub fn segment_stream_links(container: &Container, streams: &[Stream]) -> Vec<SegmentStreamLink> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    let entry_start = usize::try_from(entry_offset).expect("in-bounds directory offset");
    let mut links = Vec::new();
    for (row_ordinal, row) in index.rows.into_iter().enumerate() {
        for (slot, relative) in [
            (SegmentIndexSlot::TypeCode, row.type_code),
            (SegmentIndexSlot::SubtypeCode, row.subtype_code),
            (SegmentIndexSlot::Value, row.value),
        ] {
            let relative = relative as usize;
            let Some(wrapper) = container.data.get(entry_start + relative..) else {
                continue;
            };
            let Some(wrapper_word) = cadmpeg_ir::le::u32_at(wrapper, 0) else {
                continue;
            };
            let extension = (wrapper_word & 0x3fff_ffff) as usize;
            let wrapper_byte_len = match wrapper_word & 0xc000_0000 {
                0x8000_0000 => 8usize.checked_add(extension),
                0xc000_0000 => 33usize.checked_add(extension),
                _ => continue,
            };
            let Some(wrapper_byte_len) = wrapper_byte_len else {
                continue;
            };
            let zlib_offset = entry_start + relative + wrapper_byte_len;
            let Some((stream_ordinal, stream)) = streams
                .iter()
                .enumerate()
                .find(|(_, stream)| stream.file_offset == zlib_offset)
            else {
                continue;
            };
            links.push(SegmentStreamLink {
                id: format!("nx:segment-stream-links:link#{}", links.len()),
                row: format!("nx:segment-index:row#{row_ordinal}"),
                slot,
                stream_ordinal: stream_ordinal as u32,
                stream_kind: match stream.kind {
                    StreamKind::Partition => "partition",
                    StreamKind::Deltas => "deltas",
                    StreamKind::Plain => "plain",
                    StreamKind::Preview => "preview",
                }
                .to_string(),
                wrapper_byte_len: wrapper_byte_len as u32,
                source_offset: entry_offset + relative as u64,
            });
        }
    }
    links
}

/// Bind partition and cached-body streams to feature-history body object indices.
pub fn segment_body_bindings(container: &Container, streams: &[Stream]) -> Vec<SegmentBodyBinding> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    let words = index
        .rows
        .iter()
        .flat_map(|row| [row.type_code, row.subtype_code, row.value])
        .collect::<Vec<_>>();
    segment_stream_links(container, streams)
        .into_iter()
        .filter(|link| matches!(link.stream_kind.as_str(), "partition" | "plain"))
        .filter_map(|link| {
            let row = link.row.rsplit_once('#')?.1.parse::<usize>().ok()?;
            let slot = match link.slot {
                SegmentIndexSlot::TypeCode => 0,
                SegmentIndexSlot::SubtypeCode => 1,
                SegmentIndexSlot::Value => 2,
            };
            let pointer_word = row.checked_mul(3)?.checked_add(slot)?;
            (words.get(pointer_word + 1) == Some(&0)).then_some(())?;
            let body_object_index = *words.get(pointer_word + 2)?;
            let body_alias_object_index = *words.get(pointer_word + 3)?;
            let stream_role = *words.get(pointer_word + 4)?;
            (body_object_index != 0 && body_alias_object_index != 0).then_some(())?;
            Some(SegmentBodyBinding {
                id: format!("nx:segment-body-bindings:binding#{}", link.stream_ordinal),
                stream_link: link.id,
                stream_ordinal: link.stream_ordinal,
                stream_kind: link.stream_kind,
                body_object_index,
                body_alias_object_index,
                stream_role,
                source_offset: entry_offset + ((pointer_word + 2) * 4) as u64,
            })
        })
        .collect()
}

/// Retain named attribute-class declarations from all Parasolid streams.
pub fn parasolid_attribute_definitions(streams: &[Stream]) -> Vec<ParasolidAttributeDefinition> {
    streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::attribute_definitions(&stream.inflated)
                .into_iter()
                .map(move |definition| ParasolidAttributeDefinition {
                    id: format!(
                        "nx:s{stream_ordinal}:attribute-definition#{}",
                        definition.xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: definition.xmt,
                    name: definition.name.to_string(),
                    field_count: definition.field_count,
                    field_record_xmt: definition.field_record_xmt,
                    field_record_references: definition.field_record_references,
                    field_record_header_words: definition.field_record_header_words,
                    field_descriptor_prefix: definition.field_descriptor_prefix,
                    field_storage: parasolid_attribute_field_storage(
                        &definition.field_descriptor_prefix,
                    ),
                    field_codes: definition.field_codes.to_vec(),
                    inflated_offset: definition.offset as u64,
                })
        })
        .collect()
}

/// Retain complete typed rolling-ball blend records from all Parasolid streams.
pub fn parasolid_blend_surface_records(streams: &[Stream]) -> Vec<ParasolidBlendSurfaceRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::topology::blend_surfaces(&stream.inflated)
                .into_iter()
                .map(move |blend| ParasolidBlendSurfaceRecord {
                    id: format!("nx:s{stream_ordinal}:blend-surface-record#{}", blend.xmt),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: blend.xmt,
                    support_xmts: blend.supports,
                    spine_xmt: blend.spine,
                    offsets: blend.offsets,
                    thumb_weights: blend.thumb_weights,
                    inflated_offset: blend.pos as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Retain every non-null topology-to-attribute-list reference.
pub fn parasolid_topology_attribute_list_references(
    streams: &[Stream],
    entity_records: &[ParasolidEntity51Record],
) -> Vec<ParasolidTopologyAttributeListReference> {
    let mut records_by_identity = BTreeMap::<(u32, u32), Vec<&str>>::new();
    for record in entity_records {
        records_by_identity
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push(record.id.as_str());
    }
    let mut references = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        let graph = crate::topology::Graph::parse(&stream.inflated);
        for topology_type in [13, 14, 15, 16, 17, 18] {
            for node in graph.of_kind(topology_type) {
                let attribute_list_xmt = match topology_type {
                    13 => node.shell_fields().map(|fields| fields.attributes),
                    14 => node.face_fields().map(|fields| fields.attributes),
                    15 => node.loop_fields().map(|fields| fields.attributes),
                    16 => node.edge_fields().map(|fields| fields.attributes),
                    17 => node.fin_fields().map(|fields| fields.attributes),
                    18 => node.vertex_fields().map(|fields| fields.attributes),
                    _ => unreachable!("bounded topology family"),
                };
                let Some(attribute_list_xmt) = attribute_list_xmt.filter(|value| *value > 1) else {
                    continue;
                };
                let Some(inflated_offset) = node.attribute_field_offset() else {
                    continue;
                };
                references.push(ParasolidTopologyAttributeListReference {
                    id: format!(
                        "nx:s{stream_ordinal}:topology-attribute-list-reference#{topology_type}-{}",
                        node.xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    topology_type,
                    topology_xmt: node.xmt,
                    attribute_list_xmt,
                    attribute_list_record: records_by_identity
                        .get(&(stream_ordinal as u32, attribute_list_xmt))
                        .and_then(|records| {
                            let [record] = records.as_slice() else {
                                return None;
                            };
                            Some((*record).to_string())
                        }),
                    inflated_offset: inflated_offset as u64,
                });
            }
        }
    }
    references
}

/// Decode every framed type-81 entity/attribute-list record.
pub fn parasolid_entity_51_records(streams: &[Stream]) -> Vec<ParasolidEntity51Record> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_51_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity51Record {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-51#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    flags: record.flags,
                    sequence: record.sequence,
                    discriminator: record.discriminator,
                    references: record.references,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode every self-framed printable type-84 string record.
pub fn parasolid_entity_54_string_records(
    streams: &[Stream],
) -> Vec<ParasolidEntity54StringRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_54_string_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity54StringRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-54-string#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    value: record.value.to_string(),
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode every counted type-82 unsigned-integer record.
pub fn parasolid_entity_52_integer_records(
    streams: &[Stream],
) -> Vec<ParasolidEntity52IntegerRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_52_integer_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity52IntegerRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-52-integers#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    values: record.values,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode every counted type-83 finite binary64 record.
pub fn parasolid_entity_53_double_records(
    streams: &[Stream],
) -> Vec<ParasolidEntity53DoubleRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_53_double_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity53DoubleRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-53-doubles#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    values: record.values,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Join type-81 reference slots to unique same-stream numeric value records.
pub fn parasolid_entity_51_numeric_uses(
    entities: &[ParasolidEntity51Record],
    integers: &[ParasolidEntity52IntegerRecord],
    doubles: &[ParasolidEntity53DoubleRecord],
) -> Vec<ParasolidEntity51NumericUse> {
    let mut values = BTreeMap::<(u32, u32), Vec<(ParasolidEntity51NumericKind, &str)>>::new();
    for record in integers {
        values
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push((ParasolidEntity51NumericKind::UnsignedIntegers, &record.id));
    }
    for record in doubles {
        values
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push((ParasolidEntity51NumericKind::Doubles, &record.id));
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (reference_ordinal, referenced_xmt) in entity.references.iter().copied().enumerate() {
            let Some([(kind, value_record)]) = values
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51NumericUse {
                id: format!(
                    "nx:s{}:entity-51-numeric-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                reference_ordinal: reference_ordinal as u32,
                referenced_xmt,
                kind: *kind,
                value_record: (*value_record).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Join type-81 reference slots to unique same-stream type-84 strings.
pub fn parasolid_entity_51_string_uses(
    entities: &[ParasolidEntity51Record],
    strings: &[ParasolidEntity54StringRecord],
) -> Vec<ParasolidEntity51StringUse> {
    let mut strings_by_identity = BTreeMap::<(u32, u32), Vec<&str>>::new();
    for string in strings {
        strings_by_identity
            .entry((string.stream_ordinal, string.xmt))
            .or_default()
            .push(string.id.as_str());
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (reference_ordinal, referenced_xmt) in entity.references.iter().copied().enumerate() {
            let Some([string]) = strings_by_identity
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51StringUse {
                id: format!(
                    "nx:s{}:entity-51-string-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                reference_ordinal: reference_ordinal as u32,
                referenced_xmt,
                string_record: (*string).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Resolve topology-owned attribute instances through their class discriminator.
pub fn parasolid_topology_attribute_class_uses(
    topology_references: &[ParasolidTopologyAttributeListReference],
    class_uses: &[ParasolidAttributeClassUse],
) -> Vec<ParasolidTopologyAttributeClassUse> {
    let class_uses = class_uses
        .iter()
        .map(|class_use| (class_use.entity_51_record.as_str(), class_use))
        .collect::<BTreeMap<_, _>>();
    let mut uses = Vec::new();
    for reference in topology_references {
        let Some(entity_id) = reference.attribute_list_record.as_deref() else {
            continue;
        };
        let Some(class_use) = class_uses.get(entity_id) else {
            continue;
        };
        uses.push(ParasolidTopologyAttributeClassUse {
            id: format!(
                "nx:s{}:topology-attribute-class-use#{}-{}",
                reference.stream_ordinal, reference.topology_type, reference.topology_xmt
            ),
            topology_attribute_reference: reference.id.clone(),
            entity_51_record: class_use.entity_51_record.clone(),
            class_discriminator: class_use.class_discriminator,
            definition_xmt: class_use.definition_xmt,
            attribute_definition: class_use.attribute_definition.clone(),
        });
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Resolve every type-81 attribute instance through its class discriminator.
pub fn parasolid_attribute_class_uses(
    entities: &[ParasolidEntity51Record],
    definitions: &[ParasolidAttributeDefinition],
) -> Vec<ParasolidAttributeClassUse> {
    let mut definitions_by_identity =
        BTreeMap::<(u32, u16), Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_identity
            .entry((definition.stream_ordinal, definition.xmt))
            .or_default()
            .push(definition);
    }
    let mut uses = entities
        .iter()
        .filter_map(|entity| {
            let definition_xmt = entity.discriminator.checked_add(1)?;
            let [definition] = definitions_by_identity
                .get(&(entity.stream_ordinal, definition_xmt))?
                .as_slice()
            else {
                return None;
            };
            Some(ParasolidAttributeClassUse {
                id: format!(
                    "nx:s{}:attribute-class-use#{}-{}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                class_discriminator: entity.discriminator,
                definition_xmt,
                attribute_definition: definition.id.clone(),
            })
        })
        .collect::<Vec<_>>();
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Unit declared by an NX numeric expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionUnit {
    /// Canonical model length in millimeters.
    Millimeter,
    /// Angular value in degrees as stored by NX.
    Degree,
}

/// Named parameter declaration in a bounded NX expression object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpressionDeclaration {
    /// Globally unique declaration identity.
    pub id: String,
    /// Persistent OM object identifier.
    pub object_id: u32,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Exact NX parameter name.
    pub name: String,
    /// Decimal source parameter identifier following `p`.
    pub parameter_index: u32,
    /// Qualified role following the parameter identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,
    /// Independently framed constant numeric expression in the declaration record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,
    /// Directory entry containing the declaration record.
    pub source_entry: String,
    /// Absolute file offset of the declaration-name marker.
    pub source_offset: u64,
}

/// Explicit numeric expression serialized in one NX OM entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expression {
    /// Globally unique native-record identity.
    pub id: String,
    /// Persistent OM object identifier.
    pub object_id: Option<u32>,
    /// Owning entry in the native OM record directory, when externally bounded.
    pub record: Option<String>,
    /// Exact-name declaration record for this parameter, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<String>,
    /// NX parameter name.
    pub name: String,
    /// Decimal source parameter identifier following the leading `p`.
    pub parameter_index: Option<u32>,
    /// Qualified role following the parameter identifier.
    pub qualifier: Option<String>,
    /// Declared native unit.
    pub unit: ExpressionUnit,
    /// Exact serialized expression text.
    pub expression: String,
    /// Finite numeric value after context-free and dependency-graph evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Self-contained expression table selected by the nearest preceding table marker.
    #[serde(default)]
    pub source_table: String,
    /// Absolute file offset of the expression text.
    pub source_offset: u64,
}

/// Return exact `p<decimal>[_qualifier]` references in formula occurrence order.
pub(crate) fn expression_parameter_names(expression: &str) -> Vec<&str> {
    let bytes = expression.as_bytes();
    let mut names = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let Some(end) = expression_parameter_reference_end(bytes, at) else {
            at += 1;
            continue;
        };
        names.push(&expression[at..end]);
        at = end;
    }
    names
}

fn expression_parameter_reference_end(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'p')
        || at
            .checked_sub(1)
            .and_then(|before| bytes.get(before))
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return None;
    }
    let mut end = at + 1;
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        end += 1;
    }
    let name = std::str::from_utf8(bytes.get(at..end)?).ok()?;
    crate::om::parameter_name_parts(name).map(|_| end)
}

/// Length-framed class definition from an NX OM type registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassDefinition {
    /// Globally unique native-record identity.
    pub id: String,
    /// Registered `UGS::` class name.
    pub name: String,
    /// Zero-based declaration ordinal used as class identity.
    pub ordinal: u32,
    /// Declaration code serialized after the class name.
    pub trailing_code: u8,
    /// Exact bytes between this declaration core and the next class declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registry_suffix: Vec<u8>,
    /// Variable-width prefix of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layout_prefix: Vec<u8>,
    /// Stable eight-byte class fingerprint in a framed registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_fingerprint: Option<[u8; 8]>,
    /// Terminal byte of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_terminal: Option<u8>,
    /// Absolute file offset of the containing OM section base.
    pub section_offset: u64,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the definition's length byte.
    pub source_offset: u64,
}

/// Member declaration from an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Globally unique declaration identity.
    pub id: String,
    /// Registered `m_` member name.
    pub name: String,
    /// Zero-based declaration ordinal within its section.
    pub ordinal: u32,
    /// Declaration code serialized immediately after the name.
    pub trailing_code: u8,
    /// Exact bytes between this declaration core and the next member declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registry_suffix: Vec<u8>,
    /// Absolute file offset of the containing OM section signature.
    pub section_offset: u64,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the declaration length byte.
    pub source_offset: u64,
}

/// Directory entry for one externally bounded NX OM entity record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based record ordinal within the indexed section.
    pub record_ordinal: u32,
    /// Absolute file offset of the containing OM section base.
    pub section_offset: u64,
    /// Exact serialized record length.
    pub byte_len: u64,
    /// SHA-256 of the exact serialized record bytes.
    pub sha256: String,
    /// Ordered distinct same-section records referenced by this record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// Ordered distinct same-section records that reference this record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependents: Vec<String>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the record start.
    pub source_offset: u64,
}

/// One externally bounded block in an NX OM offset-only column store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlock {
    /// Globally unique block identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based block ordinal within the offset-only section.
    pub block_ordinal: u32,
    /// Whether this is the store control block or one data column block.
    pub role: DataBlockRole,
    /// Absolute file offset of the containing OM section base.
    pub section_offset: u64,
    /// Exact serialized block length.
    pub byte_len: u64,
    /// SHA-256 of the exact serialized block bytes.
    pub sha256: String,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the block start.
    pub source_offset: u64,
}

/// Ordered value from a zero-prefixed offset-only OM store control array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlValue {
    /// Globally unique control-value identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based word order in the complete control block.
    pub ordinal: u32,
    /// Unsigned 24-bit value serialized after the zero byte.
    pub value: u32,
    /// Absolute file offset of the four-byte word.
    pub source_offset: u64,
}

/// Ordered little-endian value preceding a control-block product record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlIndexValue {
    /// Globally unique value identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based value order in the aligned prefix array.
    pub ordinal: u32,
    /// Number of leading zero bytes before the aligned array.
    pub prefix_byte_len: u8,
    /// Unsigned little-endian value.
    pub value: u32,
    /// Same-section offset-store block addressed by an in-range value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_data_block: Option<String>,
    /// Absolute file offset of the four-byte value.
    pub source_offset: u64,
}

/// Registered class selected by the leading lane of an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlClassReference {
    /// Globally unique class-reference identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based order in the class-selection lane.
    pub ordinal: u32,
    /// Zero-based ordinal in the store's class registry.
    pub class_ordinal: u32,
    /// Target in the native `class_definitions` arena.
    pub class_definition: String,
    /// Exact registered class name.
    pub class_name: String,
    /// Absolute file offset of the four-byte control word.
    pub source_offset: u64,
}

/// Ordered object reference carried by an offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Owning block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based reference order within the block.
    pub ordinal: u32,
    /// Referenced persistent OM object ID.
    pub object_id: u32,
    /// Uniquely resolved object record in the same directory entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_record: Option<String>,
    /// Uniquely resolved parameter declaration carrying this object ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_expression_declaration: Option<String>,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Complete counted block-index lane carried by one offset-store block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockCountedIndexLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based lane order within the block.
    pub ordinal: u32,
    /// Serialized count including the anchor and terminal slot.
    pub declared_count: u8,
    /// Decoded anchoring block index.
    pub anchor_index: u32,
    /// Same-section block addressed by the anchor.
    pub anchor_data_block: String,
    /// Ordered decoded member block indices.
    pub member_indices: Vec<u32>,
    /// Ordered same-section blocks addressed by the members.
    pub member_data_blocks: Vec<String>,
    /// Absolute file offset of the opening `01` marker.
    pub source_offset: u64,
    /// Absolute file offset of the anchoring compact index.
    pub anchor_source_offset: u64,
    /// Ordered absolute file offsets of member compact indices.
    pub member_source_offsets: Vec<u64>,
}

/// Fixed-width nullable `ABR` block-reference lane in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockAbrReferenceLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based lane order within the section's column storage.
    pub ordinal: u32,
    /// Sixteen ordered nullable serialized block indices.
    pub slot_indices: Vec<Option<u32>>,
    /// Sixteen ordered nullable same-section block identities.
    pub slot_data_blocks: Vec<Option<String>>,
    /// Absolute file offsets of the sixteen compact-index tokens.
    pub slot_source_offsets: Vec<u64>,
    /// Directory entry containing the offset-only store.
    pub source_entry: String,
    /// Absolute file offset of the opening `11` marker.
    pub source_offset: u64,
}

/// Self-framed index row in contiguous offset-store column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// First non-null compact index.
    pub first_index: u32,
    /// Serialized `03` or `07` row flag.
    pub flag: u8,
    /// Four ordered non-null compact indices after the row flag.
    pub indices: [u32; 4],
    /// Four same-section blocks addressed by the compact indices.
    pub data_blocks: [String; 4],
    /// Directory entry containing the offset-only store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
    /// Absolute file offset of the opening discriminator.
    pub source_offset: u64,
    /// Absolute file offset of the first compact index.
    pub first_index_source_offset: u64,
    /// Four ordered absolute file offsets of the compact indices.
    pub index_source_offsets: [u64; 4],
}

/// Self-framed linked index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockLinkedIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// Unresolved leading compact index.
    pub first_index: u32,
    /// Serialized `16`, `17`, or `18` discriminator.
    pub discriminator: u8,
    /// Target compact block index.
    pub target_index: u32,
    /// Three compact block indices after `ff ff 90 fe`.
    pub indices: [u32; 3],
    /// Target block followed by the three post-marker blocks.
    pub data_blocks: [String; 4],
    /// Serialized `03` or `07` flag.
    pub flag: u8,
    /// Serialized `04` or `07` mode.
    pub mode: u8,
    /// Directory entry containing the store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
    /// Absolute file offset of the opening discriminator.
    pub source_offset: u64,
    /// Absolute file offset of the leading compact index.
    pub first_index_source_offset: u64,
    /// Absolute file offset of the target compact index.
    pub target_index_source_offset: u64,
    /// Absolute file offsets of the three post-marker indices.
    pub index_source_offsets: [u64; 3],
}

/// Self-framed target-index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockTargetIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// Target compact block index.
    pub target_index: u32,
    /// Three compact block indices after `ff ff 90 fe`.
    pub indices: [u32; 3],
    /// Target block followed by the three post-marker blocks.
    pub data_blocks: [String; 4],
    /// Serialized `04` or `07` mode.
    pub mode: u8,
    /// Directory entry containing the store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
    /// Absolute file offset of the opening discriminator.
    pub source_offset: u64,
    /// Absolute file offset of the target compact index.
    pub target_index_source_offset: u64,
    /// Absolute file offsets of the three post-marker indices.
    pub index_source_offsets: [u64; 3],
}

/// Complete composite table spanning linked and target-index row grammars.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockColumnIndexTable {
    /// Globally unique table identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Leading mode-7 linked row.
    pub opening_linked_row: String,
    /// Consecutive target-index rows in ascending source order.
    pub target_rows: Vec<String>,
    /// Consecutive mode-4 linked rows in ascending source order.
    pub linked_rows: Vec<String>,
    /// First and greatest target block ordinal.
    pub first_target_index: u32,
    /// Last and least target block ordinal.
    pub last_target_index: u32,
    /// Directory entry containing the store.
    pub source_entry: String,
    /// Absolute source offset of the opening linked row.
    pub source_offset: u64,
}

/// Product/version header from one indexed NX OM store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreHeader {
    /// Globally unique store-header identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Persistent object identity when the header belongs to an ID-bounded record.
    pub object_id: Option<u32>,
    /// Exact printable product/version text.
    pub version: String,
    /// Directory entry containing the OM store.
    pub source_entry: String,
    /// Absolute file offset of the `04 01` marker.
    pub source_offset: u64,
}

/// Role of one bounded block in an offset-only NX OM store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataBlockRole {
    /// Store-level schema and root metadata from boundary slot zero.
    Control,
    /// One offset-bounded column-storage block.
    Column,
}

/// Self-framed printable string carried by one NX OM record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringValue {
    /// Globally unique value identity.
    pub id: String,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based occurrence ordinal within the owning record.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: String,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the `66 32 03` marker.
    pub source_offset: u64,
}

/// Tagged reference family serialized in an NX OM record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectReferenceKind {
    /// `e0` marker followed by a 32-bit persistent handle.
    PersistentHandle,
    /// Four-byte `0xC?` tagged 28-bit reference.
    Tagged28,
    /// Count-framed `90` reference to a record ordinal in the same section.
    RecordOrdinal16,
}

/// Ordered tagged-reference occurrence owned by one NX OM record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectReference {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based occurrence ordinal within the owning record.
    pub ordinal: u32,
    /// Tagged reference family.
    pub kind: ObjectReferenceKind,
    /// Reference value without marker/tag bits.
    pub value: u32,
    /// Resolved target in the native OM record directory.
    pub target_record: Option<String>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the reference marker.
    pub source_offset: u64,
}

/// Ordered persistent or tagged reference in an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlReference {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based retained-reference order within the control block.
    pub ordinal: u32,
    /// Tagged reference family.
    pub kind: ObjectReferenceKind,
    /// Reference value without marker or tag bits.
    pub value: u32,
    /// Absolute file offset of the reference marker.
    pub source_offset: u64,
}

/// Exact two-token persistent-handle run in an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlHandlePair {
    /// Globally unique pair identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// First handle-reference occurrence.
    pub first_reference: String,
    /// Second handle-reference occurrence.
    pub second_reference: String,
    /// First persistent-handle value.
    pub first_handle: u32,
    /// Second persistent-handle value.
    pub second_handle: u32,
    /// Absolute file offset of the first `e0` marker.
    pub source_offset: u64,
}

/// Cross-record identity established by equal persistent-handle values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentHandle {
    /// Globally unique handle identity.
    pub id: String,
    /// Unsigned persistent-handle value.
    pub value: u32,
    /// Ordered distinct OM directory records containing the handle.
    pub records: Vec<String>,
    /// Total serialized occurrences across OM records and offset-store control blocks.
    pub occurrence_count: u32,
    /// Ordered distinct offset-store control blocks containing the handle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_blocks: Vec<String>,
    /// Ordered distinct EXTREFSTREAM records containing the same handle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_records: Vec<String>,
    /// Total serialized occurrences across EXTREFSTREAM record prefixes and tails.
    #[serde(default)]
    pub external_occurrence_count: u32,
}

/// Named NX arrangement from `/Root/part/arrangements`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Configuration {
    /// Globally unique native-record identity.
    pub id: String,
    /// Arrangement name.
    pub name: String,
    /// Whether NX marks this arrangement as the default.
    pub active: bool,
    /// Directory entry containing the arrangement XML.
    pub source_entry: String,
    /// Absolute file offset of the arrangement element.
    pub source_offset: u64,
}

/// Exact agreement between the default arrangement and the part attribute
/// naming the active arrangement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationAttributeUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Default arrangement from the native configuration arena.
    pub configuration: String,
    /// Typed `NX_Arrangement` part attribute carrying the same name.
    pub part_attribute: String,
    /// Exact shared arrangement name.
    pub name: String,
}

/// One typed part-level attribute from `/Root/part/attrs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartAttribute {
    /// Globally unique native-record identity.
    pub id: String,
    /// Attribute owner token.
    pub owner: String,
    /// UTF-8 attribute title.
    pub title: String,
    /// UTF-8 attribute value.
    pub value: String,
    /// XML schema type token.
    pub value_type: String,
    /// Whether product-data management owns the value.
    pub pdm_based: bool,
    /// Attribute record schema version.
    pub version: u32,
    /// Directory entry containing the attribute XML.
    pub source_entry: String,
    /// Absolute file offset of the attribute element.
    pub source_offset: u64,
}

/// End-anchored child-part string from an NX external-reference stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReference {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based string-table ordinal within the stream.
    pub ordinal: u32,
    /// Exact serialized child-part name or path.
    pub path: String,
    /// Directory entry containing the external-reference stream.
    pub source_entry: String,
    /// Absolute file offset of the first path byte.
    pub source_offset: u64,
}

/// Externally bounded record retained from an EXTREFSTREAM index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceIndexedRecord {
    /// Globally unique indexed-record identity.
    pub id: String,
    /// Record type from the external-reference directory.
    pub record_id: u32,
    /// Exact serialized record length.
    pub byte_len: u64,
    /// SHA-256 of the exact serialized record bytes.
    pub sha256: String,
    /// Specialized handle-set record when that complete grammar resolves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle_set_record: Option<String>,
    /// Directory entry containing the external-reference stream.
    pub source_entry: String,
    /// Absolute file offset of the indexed record.
    pub source_offset: u64,
}

/// Indexed EXTREFSTREAM record prefix with its exact handle membership set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Record type from the external-reference directory.
    pub record_id: u32,
    /// Count declared before the four ID slots.
    pub declared_count: u16,
    /// Four uninterpreted little-endian ID slots.
    pub id_slots: [u32; 4],
    /// Strictly ascending persistent handles; the serialized closing duplicate is omitted.
    pub handles: Vec<u32>,
    /// Whether the final serialized handle repeats the preceding handle.
    pub closing_duplicate: bool,
    /// Length of the decoded record prefix.
    pub prefix_byte_len: u64,
    /// Length after the decoded handle-set prefix and before the next record or string table.
    pub tail_byte_len: u64,
    /// Directory entry containing the external-reference stream.
    pub source_entry: String,
    /// Absolute file offset of the record marker.
    pub source_offset: u64,
}

/// Empty EXTREFSTREAM indexed-record form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceEmptyRecord {
    /// Globally unique empty-record identity.
    pub id: String,
    /// Owning record in the native `external_reference_indexed_records` arena.
    pub indexed_record: String,
    /// Whether the six-byte header is followed by a closing `01` marker.
    pub closing_marker: bool,
}

/// Exact adjacent reference pair in an EXTREFSTREAM handle-set tail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceTailReferencePair {
    /// Globally unique pair identity.
    pub id: String,
    /// Owning record in the native `external_reference_records` arena.
    pub handle_set_record: String,
    /// Zero-based pair order within the bounded tail.
    pub ordinal: u32,
    /// Persistent handle from the `e0 + u32 BE` token.
    pub persistent_handle: u32,
    /// Low 28 bits of the following four-byte `0xC?` reference.
    pub tagged_reference: u32,
    /// Absolute file offset of the `e0` marker.
    pub source_offset: u64,
}

/// One external-reference record slot resolved through its same-stream string table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceRecordStringUse {
    /// Globally unique slot-use identity.
    pub id: String,
    /// Owning record in the native `external_reference_records` arena.
    pub external_record: String,
    /// Zero-based slot in the record's four-value lane.
    pub slot: u8,
    /// Serialized string-table index.
    pub string_index: u32,
    /// Target in the native `external_references` arena.
    pub external_reference: String,
    /// Absolute file offset of the serialized `u32 LE` slot value.
    pub source_offset: u64,
}

/// Child-part identity selected by one complete external-reference record lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceRecordChild {
    /// Globally unique child-binding identity.
    pub id: String,
    /// Owning record in the native `external_reference_records` arena.
    pub external_record: String,
    /// Slot-zero child filename in the native `external_references` arena.
    pub name_reference: String,
    /// Slot-two child directory in the native `external_references` arena.
    pub directory_reference: String,
}

/// Embedded NX material texture stored as a TIFF stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialTextureAsset {
    /// Globally unique native-record identity.
    pub id: String,
    /// Texture stream leaf name carried by the directory path.
    pub name: String,
    /// TIFF byte order: `little_endian` or `big_endian`.
    pub byte_order: String,
    /// TIFF format version. NX material textures use version 42.
    pub version: u16,
    /// Absolute byte offset of the first TIFF image-file directory, relative to the asset payload.
    pub first_ifd_offset: u32,
    /// Exact texture payload length.
    pub byte_len: u64,
    /// SHA-256 digest of the exact TIFF payload.
    pub sha256: String,
    /// Directory entry containing the texture.
    pub source_entry: String,
    /// Absolute file offset of the TIFF header.
    pub source_offset: u64,
}

/// Exact QAF catalog mapping for one embedded material texture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialTextureCatalogEntry {
    /// Globally unique native relation identity.
    pub id: String,
    /// Target in the native `material_texture_assets` arena.
    pub texture_asset: String,
    /// Stored path relative to `/Root/`.
    pub storage_path: String,
    /// Logical material-texture path recorded by QAF metadata.
    pub material_path: String,
    /// Exact QAF creation-time text.
    pub create_time: String,
    /// Exact QAF modification-time text.
    pub modify_time: String,
    /// Directory entry containing the QAF catalog.
    pub source_entry: String,
    /// Absolute file offset of the `folderProperties` element.
    pub source_offset: u64,
}

/// Decode every strictly framed TIFF material-texture directory entry.
pub fn material_texture_assets(container: &Container) -> Vec<MaterialTextureAsset> {
    let mut assets = container
        .entries
        .iter()
        .filter_map(|entry| {
            let name = entry.name.strip_prefix("/Root/materialsTif/")?;
            (!name.is_empty()).then_some(())?;
            let (offset, size) = entry.file_span?;
            let (start, size) = (usize::try_from(offset).ok()?, usize::try_from(size).ok()?);
            let payload = container.data.get(start..start.checked_add(size)?)?;
            let (byte_order, version, first_ifd_offset) = match payload.get(..8)? {
                [b'I', b'I', 42, 0, a, b, c, d] => {
                    ("little_endian", 42, u32::from_le_bytes([*a, *b, *c, *d]))
                }
                [b'M', b'M', 0, 42, a, b, c, d] => {
                    ("big_endian", 42, u32::from_be_bytes([*a, *b, *c, *d]))
                }
                _ => return None,
            };
            let first_ifd = usize::try_from(first_ifd_offset).ok()?;
            (first_ifd >= 8 && first_ifd < payload.len()).then_some(())?;
            Some(MaterialTextureAsset {
                id: String::new(),
                name: name.to_string(),
                byte_order: byte_order.to_string(),
                version,
                first_ifd_offset,
                byte_len: size as u64,
                sha256: sha256_hex(payload),
                source_entry: entry.name.clone(),
                source_offset: offset,
            })
        })
        .collect::<Vec<_>>();
    assets.sort_by(|first, second| first.source_entry.cmp(&second.source_entry));
    for (ordinal, asset) in assets.iter_mut().enumerate() {
        asset.id = format!("nx:container:material-texture#{ordinal}");
    }
    assets
}

/// Join QAF material paths to embedded TIFF streams by exact stored path.
pub fn material_texture_catalog_entries(
    container: &Container,
    assets: &[MaterialTextureAsset],
) -> Vec<MaterialTextureCatalogEntry> {
    let Some((entry_index, entry)) = container
        .entries
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.name == "/Root/qafmetadata")
    else {
        return Vec::new();
    };
    let Some((entry_offset, size)) = entry.file_span else {
        return Vec::new();
    };
    let Some(start) = usize::try_from(entry_offset).ok() else {
        return Vec::new();
    };
    let Some(size) = usize::try_from(size).ok() else {
        return Vec::new();
    };
    let Some(end) = start.checked_add(size) else {
        return Vec::new();
    };
    let Some(payload) = container.data.get(start..end) else {
        return Vec::new();
    };
    let Some(entries) =
        parse_material_texture_catalog(payload, entry_index, &entry.name, entry_offset, assets)
    else {
        return Vec::new();
    };
    entries
}

fn parse_material_texture_catalog(
    payload: &[u8],
    entry_index: usize,
    source_entry: &str,
    entry_offset: u64,
    assets: &[MaterialTextureAsset],
) -> Option<Vec<MaterialTextureCatalogEntry>> {
    let document = roxmltree::Document::parse(xml_stream_text(payload)?).ok()?;
    let root = document.root_element();
    (root.tag_name().name() == "folderContents").then_some(())?;
    let assets_by_path = assets
        .iter()
        .map(|asset| Some((asset.source_entry.strip_prefix("/Root/")?, asset)))
        .collect::<Option<BTreeMap<_, _>>>()?;
    let mut catalog = Vec::new();
    let mut seen_assets = BTreeSet::new();
    for node in root.children().filter(roxmltree::Node::is_element) {
        (node.tag_name().name() == "folderProperties").then_some(())?;
        let storage_path = node.attribute("location")?;
        let material_path = node.attribute("unmappedLocation")?;
        let children = node
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>();
        let [create, modify] = children.as_slice() else {
            return None;
        };
        (create.tag_name().name() == "createTime" && modify.tag_name().name() == "modifyTime")
            .then_some(())?;
        let create_time = create.text()?;
        let modify_time = modify.text()?;
        if !storage_path.starts_with("materialsTif/") {
            continue;
        }
        let asset = assets_by_path.get(storage_path)?;
        material_path
            .strip_prefix("materialsTif/")
            .filter(|name| !name.is_empty())?;
        seen_assets.insert(asset.id.as_str()).then_some(())?;
        let ordinal = catalog.len();
        catalog.push(MaterialTextureCatalogEntry {
            id: format!("nx:qafmetadata-{entry_index}:material-texture#{ordinal}"),
            texture_asset: asset.id.clone(),
            storage_path: storage_path.to_string(),
            material_path: material_path.to_string(),
            create_time: create_time.to_string(),
            modify_time: modify_time.to_string(),
            source_entry: source_entry.to_string(),
            source_offset: entry_offset + node.range().start as u64,
        });
    }
    Some(catalog)
}

/// Decode end-anchored external child-part string tables.
pub fn external_references(container: &Container) -> Vec<ExternalReference> {
    let mut ordinals = BTreeMap::<String, u32>::new();
    container
        .external_reference_strings()
        .into_iter()
        .map(|(entry, relative, path)| {
            let ordinal = ordinals.entry(entry.name.clone()).or_default();
            let current = *ordinal;
            *ordinal += 1;
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            ExternalReference {
                id: format!("nx:external-reference:{}#{current}", entry.name),
                ordinal: current,
                path,
                source_entry: entry.name.clone(),
                source_offset: entry_offset + relative as u64,
            }
        })
        .collect()
}

/// Decode exact indexed external-reference record prefixes.
pub fn external_reference_records(container: &Container) -> Vec<ExternalReferenceRecord> {
    container
        .external_reference_records()
        .into_iter()
        .map(|(entry, record)| {
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            ExternalReferenceRecord {
                id: format!(
                    "nx:external-reference-record:{}#{}",
                    entry.name, record.record_id
                ),
                record_id: record.record_id,
                declared_count: record.declared_count,
                id_slots: record.id_slots,
                handles: record.handles,
                closing_duplicate: record.closing_duplicate,
                prefix_byte_len: record.prefix_byte_len as u64,
                tail_byte_len: record.tail_byte_len as u64,
                source_entry: entry.name.clone(),
                source_offset: entry_offset + record.offset as u64,
            }
        })
        .collect()
}

/// Retain all indexed records and link uniquely decoded handle-set records.
pub fn external_reference_indexed_records(
    container: &Container,
    decoded: &[ExternalReferenceRecord],
) -> Vec<ExternalReferenceIndexedRecord> {
    let mut decoded_by_key = BTreeMap::<(&str, u32), Option<&ExternalReferenceRecord>>::new();
    for record in decoded {
        decoded_by_key
            .entry((record.source_entry.as_str(), record.record_id))
            .and_modify(|value| *value = None)
            .or_insert(Some(record));
    }
    container
        .external_reference_indexed_records()
        .into_iter()
        .filter_map(|(entry, record)| {
            let entry_offset = entry.file_span?.0;
            let source_offset = entry_offset.checked_add(record.offset as u64)?;
            let start = usize::try_from(source_offset).ok()?;
            let bytes = container
                .data
                .get(start..start.checked_add(record.byte_len)?)?;
            Some(ExternalReferenceIndexedRecord {
                id: format!(
                    "nx:external-reference-indexed-record:{}#{}",
                    entry.name, record.record_id
                ),
                record_id: record.record_id,
                byte_len: record.byte_len as u64,
                sha256: sha256_hex(bytes),
                handle_set_record: decoded_by_key
                    .get(&(entry.name.as_str(), record.record_id))
                    .and_then(|record| *record)
                    .map(|record| record.id.clone()),
                source_entry: entry.name.clone(),
                source_offset,
            })
        })
        .collect()
}

/// Decode every exact six- or seven-byte empty indexed record.
pub fn external_reference_empty_records(
    container: &Container,
    indexed: &[ExternalReferenceIndexedRecord],
) -> Vec<ExternalReferenceEmptyRecord> {
    indexed
        .iter()
        .filter_map(|record| {
            let start = usize::try_from(record.source_offset).ok()?;
            let length = usize::try_from(record.byte_len).ok()?;
            let bytes = container.data.get(start..start.checked_add(length)?)?;
            let closing_marker = crate::container::parse_extref_empty_record(bytes)?;
            Some(ExternalReferenceEmptyRecord {
                id: record.id.replacen("indexed-record", "empty-record", 1),
                indexed_record: record.id.clone(),
                closing_marker,
            })
        })
        .collect()
}

/// Decode exact adjacent reference pairs from bounded handle-set tails.
pub fn external_reference_tail_reference_pairs(
    container: &Container,
    records: &[ExternalReferenceRecord],
) -> Vec<ExternalReferenceTailReferencePair> {
    records
        .iter()
        .flat_map(|record| {
            let Some(start) = usize::try_from(record.source_offset)
                .ok()
                .and_then(|start| start.checked_add(usize::try_from(record.prefix_byte_len).ok()?))
            else {
                return Vec::new();
            };
            let Some(length) = usize::try_from(record.tail_byte_len).ok() else {
                return Vec::new();
            };
            let Some(end) = start.checked_add(length) else {
                return Vec::new();
            };
            let Some(bytes) = container.data.get(start..end) else {
                return Vec::new();
            };
            crate::container::parse_extref_reference_pairs(bytes)
                .into_iter()
                .enumerate()
                .map(|(ordinal, (offset, persistent_handle, tagged_reference))| {
                    let record_key = record
                        .id
                        .split_once('#')
                        .map_or(record.id.as_str(), |(_, key)| key);
                    ExternalReferenceTailReferencePair {
                        id: format!(
                            "nx:external-reference:tail-reference-pair#{record_key}-{ordinal}"
                        ),
                        handle_set_record: record.id.clone(),
                        ordinal: ordinal as u32,
                        persistent_handle,
                        tagged_reference,
                        source_offset: (start + offset) as u64,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Resolve complete four-slot record lanes through same-stream string tables.
pub fn external_reference_record_string_uses(
    records: &[ExternalReferenceRecord],
    references: &[ExternalReference],
) -> Vec<ExternalReferenceRecordStringUse> {
    let mut references_by_key = BTreeMap::<(&str, u32), Option<&ExternalReference>>::new();
    for reference in references {
        references_by_key
            .entry((reference.source_entry.as_str(), reference.ordinal))
            .and_modify(|value| *value = None)
            .or_insert(Some(reference));
    }
    records
        .iter()
        .flat_map(|record| {
            if record.source_offset.checked_add(19).is_none() {
                return Vec::new();
            }
            let resolved = record
                .id_slots
                .iter()
                .map(|index| {
                    references_by_key
                        .get(&(record.source_entry.as_str(), *index))
                        .and_then(|reference| *reference)
                })
                .collect::<Option<Vec<_>>>();
            let Some(resolved) = resolved else {
                return Vec::new();
            };
            resolved
                .into_iter()
                .enumerate()
                .map(|(slot, reference)| {
                    let record_key = record
                        .id
                        .split_once('#')
                        .map_or(record.id.as_str(), |(_, key)| key);
                    ExternalReferenceRecordStringUse {
                        id: format!("nx:external-reference:record-string-use#{record_key}-{slot}"),
                        external_record: record.id.clone(),
                        slot: slot as u8,
                        string_index: record.id_slots[slot],
                        external_reference: reference.id.clone(),
                        source_offset: record.source_offset + 7 + slot as u64 * 4,
                    }
                })
                .collect()
        })
        .collect()
}

/// Bind complete record lanes to their slot-zero name and slot-two directory.
pub fn external_reference_record_children(
    records: &[ExternalReferenceRecord],
    references: &[ExternalReference],
    uses: &[ExternalReferenceRecordStringUse],
) -> Vec<ExternalReferenceRecordChild> {
    let mut references_by_id = BTreeMap::<&str, Option<&ExternalReference>>::new();
    for reference in references {
        references_by_id
            .entry(reference.id.as_str())
            .and_modify(|value| *value = None)
            .or_insert(Some(reference));
    }
    records
        .iter()
        .filter_map(|record| {
            let mut record_uses = uses
                .iter()
                .filter(|use_| use_.external_record == record.id)
                .collect::<Vec<_>>();
            record_uses.sort_by_key(|use_| use_.slot);
            let [slot0, slot1, slot2, slot3] = record_uses.as_slice() else {
                return None;
            };
            if [slot0.slot, slot1.slot, slot2.slot, slot3.slot] != [0, 1, 2, 3] {
                return None;
            }
            let resolved = record_uses
                .iter()
                .enumerate()
                .map(|(slot, use_)| {
                    let reference = references_by_id
                        .get(use_.external_reference.as_str())
                        .and_then(|reference| *reference)?;
                    (use_.string_index == record.id_slots[slot]
                        && reference.source_entry == record.source_entry
                        && reference.ordinal == use_.string_index)
                        .then_some(reference)
                })
                .collect::<Option<Vec<_>>>()?;
            let name = resolved[0];
            let directory = resolved[2];
            name.path
                .to_ascii_lowercase()
                .ends_with(".prt")
                .then_some(())?;
            (!directory.path.is_empty()).then_some(())?;
            Some(ExternalReferenceRecordChild {
                id: format!("{}:child", record.id),
                external_record: record.id.clone(),
                name_reference: name.id.clone(),
                directory_reference: directory.id.clone(),
            })
        })
        .collect()
}

/// Decode the explicit NX arrangement table.
pub fn configurations(container: &Container) -> Vec<Configuration> {
    if container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/part/arrangements")
        .count()
        != 1
    {
        return Vec::new();
    }
    container
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.name == "/Root/part/arrangements")
        .filter_map(|(entry_index, entry)| {
            let (offset, size) = entry.file_span?;
            let (offset_usize, size) = (usize::try_from(offset).ok()?, usize::try_from(size).ok()?);
            let payload = container
                .data
                .get(offset_usize..offset_usize.checked_add(size)?)?;
            let xml = xml_stream_text(payload)?;
            let document = roxmltree::Document::parse(xml).ok()?;
            let root = document.root_element();
            if root.tag_name().name() != "Arrangements" {
                return None;
            }

            let mut active_count = 0usize;
            let mut names = BTreeSet::new();
            let mut configurations = Vec::new();
            for (ordinal, node) in root
                .children()
                .filter(roxmltree::Node::is_element)
                .enumerate()
            {
                if node.tag_name().name() != "Arrangement" {
                    return None;
                }
                let name = node.attribute("Name")?;
                if name.is_empty() || !names.insert(name) {
                    return None;
                }
                let active = match node.attribute("Default")? {
                    "YES" => true,
                    "NO" => false,
                    _ => return None,
                };
                active_count += usize::from(active);
                configurations.push(Configuration {
                    id: format!("nx:arrangements-{entry_index}:configuration#{ordinal}"),
                    name: name.to_string(),
                    active,
                    source_entry: entry.name.clone(),
                    source_offset: offset + node.range().start as u64,
                });
            }
            (!configurations.is_empty() && active_count <= 1).then_some(configurations)
        })
        .flatten()
        .collect()
}

/// Join the two independently framed active-arrangement declarations.
pub fn configuration_attribute_uses(
    configurations: &[Configuration],
    attributes: &[PartAttribute],
) -> Vec<ConfigurationAttributeUse> {
    let active = configurations
        .iter()
        .filter(|configuration| configuration.active)
        .collect::<Vec<_>>();
    let declarations = attributes
        .iter()
        .filter(|attribute| {
            attribute.owner == "part"
                && attribute.title == "NX_Arrangement"
                && attribute.value_type == "StringAttributeType"
        })
        .collect::<Vec<_>>();
    let ([configuration], [attribute]) = (active.as_slice(), declarations.as_slice()) else {
        return Vec::new();
    };
    if configuration.name != attribute.value {
        return Vec::new();
    }
    vec![ConfigurationAttributeUse {
        id: "nx:arrangements:active-attribute-use#0".to_string(),
        configuration: configuration.id.clone(),
        part_attribute: attribute.id.clone(),
        name: configuration.name.clone(),
    }]
}

/// Decode the typed part-attribute XML stream atomically.
pub fn part_attributes(container: &Container) -> Vec<PartAttribute> {
    if container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/part/attrs")
        .count()
        != 1
    {
        return Vec::new();
    }
    container
        .entries
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.name == "/Root/part/attrs")
        .and_then(|(entry_index, entry)| {
            let (offset, size) = entry.file_span?;
            let start = usize::try_from(offset).ok()?;
            let payload = container
                .data
                .get(start..start.checked_add(usize::try_from(size).ok()?)?)?;
            parse_part_attributes(payload, entry_index, &entry.name, offset)
        })
        .unwrap_or_default()
}

pub(crate) fn parse_part_attributes(
    payload: &[u8],
    entry_index: usize,
    source_entry: &str,
    entry_offset: u64,
) -> Option<Vec<PartAttribute>> {
    let document = roxmltree::Document::parse(xml_stream_text(payload)?).ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "UgAttributes"
        || root.attribute("version")?.parse::<u32>().ok()? < 4
    {
        return None;
    }
    root.children()
        .filter(roxmltree::Node::is_element)
        .enumerate()
        .map(|(ordinal, node)| {
            if node.tag_name().name() != "Attribute" {
                return None;
            }
            Some(PartAttribute {
                id: format!("nx:part-attributes-{entry_index}:attribute#{ordinal}"),
                owner: node.attribute("owner")?.to_string(),
                title: node
                    .attribute("utf8title")
                    .or_else(|| node.attribute("title"))?
                    .to_string(),
                value: node
                    .attribute("utf8value")
                    .or_else(|| node.attribute("value"))?
                    .to_string(),
                value_type: node.attribute("type")?.to_string(),
                pdm_based: match node.attribute("pdmBased")? {
                    "true" => true,
                    "false" => false,
                    _ => return None,
                },
                version: node.attribute("version")?.parse().ok()?,
                source_entry: source_entry.to_string(),
                source_offset: entry_offset + node.range().start as u64,
            })
        })
        .collect()
}

/// Return the exact XML document carried by an NX XML stream.
///
/// NX permits one C-string terminator after the document. A terminator inside
/// the document or more than one trailing terminator rejects the whole stream.
fn xml_stream_text(payload: &[u8]) -> Option<&str> {
    let document = if let Some(document) = payload.strip_suffix(&[0]) {
        (!document.ends_with(&[0])).then_some(document)?
    } else {
        payload
    };
    (!document.contains(&0)).then_some(())?;
    std::str::from_utf8(document).ok()
}

/// Decode class definitions from every framed OM section.
pub fn class_definitions(container: &Container) -> Vec<ClassDefinition> {
    let mut definitions = BTreeMap::new();
    for (entry, section) in container.om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("OM entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, definition) in section.types.into_iter().enumerate() {
            let (layout_prefix, schema_fingerprint, layout_terminal) =
                class_layout_fields(definition.registry_suffix);
            definitions.insert(
                (entry_index, definition.offset),
                ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    layout_prefix,
                    schema_fingerprint,
                    layout_terminal,
                    section_offset: entry_offset + section.offset as u64,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                },
            );
        }
    }
    for (entry, section) in container.indexed_om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("indexed entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset + section.base_offset() as u64;
        for (ordinal, definition) in section.types.into_iter().enumerate() {
            let (layout_prefix, schema_fingerprint, layout_terminal) =
                class_layout_fields(definition.registry_suffix);
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    layout_prefix,
                    schema_fingerprint,
                    layout_terminal,
                    section_offset,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                });
        }
    }
    definitions.into_values().collect()
}

fn class_layout_fields(suffix: &[u8]) -> (Vec<u8>, Option<[u8; 8]>, Option<u8>) {
    if !(11..=14).contains(&suffix.len()) {
        return (Vec::new(), None, None);
    }
    let fingerprint_start = suffix.len() - 9;
    (
        suffix[..fingerprint_start].to_vec(),
        suffix[fingerprint_start..fingerprint_start + 8]
            .try_into()
            .ok(),
        suffix.last().copied(),
    )
}

/// Decode member definitions from every framed OM section.
pub fn field_definitions(container: &Container) -> Vec<FieldDefinition> {
    let mut definitions = BTreeMap::new();
    for (entry, section) in container.om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("OM entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, definition) in section.fields.into_iter().enumerate() {
            definitions.insert(
                (entry_index, definition.offset),
                FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    section_offset: entry_offset + section.offset as u64,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                },
            );
        }
    }
    for (entry, section) in container.indexed_om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("indexed entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset + section.base_offset() as u64;
        for (ordinal, definition) in section.fields.into_iter().enumerate() {
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    section_offset,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                });
        }
    }
    definitions.into_values().collect()
}

/// Catalog every externally bounded NX OM entity record.
pub fn object_records(container: &Container) -> Vec<ObjectRecord> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_none())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_offset = entry_offset + section.base_offset() as u64;
            let mut dependencies = BTreeMap::<usize, Vec<usize>>::new();
            let mut dependents = BTreeMap::<usize, Vec<usize>>::new();
            for (source, _, _, reference) in section.references() {
                if reference.kind != crate::om::ReferenceKind::RecordOrdinal16 {
                    continue;
                }
                let target = reference.value as usize;
                let outgoing = dependencies.entry(source).or_default();
                if !outgoing.contains(&target) {
                    outgoing.push(target);
                }
                let incoming = dependents.entry(target).or_default();
                if !incoming.contains(&source) {
                    incoming.push(source);
                }
            }
            section
                .records
                .into_iter()
                .enumerate()
                .map(move |(record_ordinal, record)| {
                    let record_id = |ordinal| {
                        format!("nx:om-record-directory-{section_ordinal}:entry#{ordinal}")
                    };
                    ObjectRecord {
                        id: record_id(record_ordinal),
                        object_id: record.object_id,
                        section_ordinal: section_ordinal as u32,
                        record_ordinal: record_ordinal as u32,
                        section_offset,
                        byte_len: record.bytes.len() as u64,
                        sha256: cadmpeg_ir::hash::sha256_hex(record.bytes),
                        dependencies: dependencies
                            .get(&record_ordinal)
                            .into_iter()
                            .flatten()
                            .map(|ordinal| record_id(*ordinal))
                            .collect(),
                        dependents: dependents
                            .get(&record_ordinal)
                            .into_iter()
                            .flatten()
                            .map(|ordinal| record_id(*ordinal))
                            .collect(),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + record.offset as u64,
                    }
                })
                .collect()
        })
        .collect()
}

/// Catalog every externally bounded block in offset-only NX OM storage.
pub fn data_blocks(container: &Container) -> Vec<DataBlock> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_some())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_offset = entry_offset + section.base_offset() as u64;
            let mut source_blocks = Vec::with_capacity(section.records.len() + 1);
            if let Some(control) = section.control {
                source_blocks.push((DataBlockRole::Control, control));
            }
            source_blocks.extend(
                section
                    .records
                    .into_iter()
                    .map(|block| (DataBlockRole::Column, block)),
            );
            source_blocks
                .into_iter()
                .enumerate()
                .map(move |(block_ordinal, (role, block))| DataBlock {
                    id: format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
                    section_ordinal: section_ordinal as u32,
                    block_ordinal: block_ordinal as u32,
                    role,
                    section_offset,
                    byte_len: block.bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(block.bytes),
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + block.offset as u64,
                })
                .collect()
        })
        .collect()
}

/// Decode complete zero-prefixed control arrays from offset-only OM stores.
pub fn data_block_control_values(container: &Container) -> Vec<DataBlockControlValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let Some(values) = crate::om::offset_store_control_values(control.bytes) else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            values
                .into_iter()
                .enumerate()
                .map(|(ordinal, value)| DataBlockControlValue {
                    id: format!(
                        "nx:om-data-block-control-values-{section_ordinal}:value#{ordinal}"
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    value,
                    source_offset: entry_offset + control.offset as u64 + ordinal as u64 * 4,
                })
                .collect()
        })
        .collect()
}

/// Resolve each atomic leading control lane through its store-local class registry.
pub fn data_block_control_class_references(
    container: &Container,
) -> Vec<DataBlockControlClassReference> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let registry = section.types;
            let Some(ordinals) = crate::om::offset_store_control_class_ordinals(
                control.bytes,
                registry.len(),
            ) else {
                return Vec::new();
            };
            let entry_index = container
                .entries
                .iter()
                .position(|candidate| std::ptr::eq(candidate, entry))
                .expect("indexed entry belongs to container");
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            ordinals
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, class_ordinal)| {
                    let definition = registry.get(usize::try_from(class_ordinal).ok()?)?;
                    Some(DataBlockControlClassReference {
                        id: format!(
                            "nx:om-data-block-control-class-references-{section_ordinal}:class#{ordinal}"
                        ),
                        data_block: data_block.clone(),
                        ordinal: ordinal as u32,
                        class_ordinal,
                        class_definition: format!(
                            "nx:om-entry-{entry_index}:class#{}",
                            definition.offset
                        ),
                        class_name: definition.name.to_string(),
                        source_offset: entry_offset + control.offset as u64 + ordinal as u64 * 4,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode aligned index arrays ending at a unique control-block product record.
pub fn data_block_control_index_values(container: &Container) -> Vec<DataBlockControlIndexValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let Some((prefix_byte_len, values)) =
                crate::om::offset_store_index_values(control.bytes)
            else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            let block_count = section.records.len() + 1;
            values
                .into_iter()
                .enumerate()
                .map(|(ordinal, value)| DataBlockControlIndexValue {
                    id: format!(
                        "nx:om-data-block-control-index-values-{section_ordinal}:value#{ordinal}"
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    prefix_byte_len: prefix_byte_len as u8,
                    value,
                    target_data_block: control_index_data_block(
                        section_ordinal,
                        block_count,
                        value,
                    ),
                    source_offset: entry_offset
                        + control.offset as u64
                        + prefix_byte_len as u64
                        + ordinal as u64 * 4,
                })
                .collect()
        })
        .collect()
}

pub(crate) fn control_index_data_block(
    section_ordinal: usize,
    block_count: usize,
    value: u32,
) -> Option<String> {
    let ordinal = usize::try_from(value)
        .ok()
        .filter(|ordinal| *ordinal < block_count)?;
    Some(format!(
        "nx:om-data-blocks-{section_ordinal}:block#{ordinal}"
    ))
}

fn column_storage_block_at(
    section_ordinal: usize,
    records: &[crate::om::EntityRecord<'_>],
    offset: usize,
) -> Option<(String, u32)> {
    records.iter().enumerate().find_map(|(ordinal, record)| {
        let block_offset = offset.checked_sub(record.offset)?;
        if block_offset >= record.bytes.len() {
            return None;
        }
        let block_offset = u32::try_from(block_offset).ok()?;
        Some((
            format!("nx:om-data-blocks-{section_ordinal}:block#{}", ordinal + 1),
            block_offset,
        ))
    })
}

/// Decode persistent-handle and tagged-28 occurrences in bounded control blocks.
pub fn data_block_control_references(container: &Container) -> Vec<DataBlockControlReference> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            crate::om::references(control.bytes, control.offset)
                .into_iter()
                .filter(|reference| reference.kind != crate::om::ReferenceKind::RecordOrdinal16)
                .enumerate()
                .map(|(ordinal, reference)| DataBlockControlReference {
                    id: format!(
                        "nx:om-data-block-control-references-{section_ordinal}:reference#{}",
                        reference.offset
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    kind: match reference.kind {
                        crate::om::ReferenceKind::PersistentHandle => {
                            ObjectReferenceKind::PersistentHandle
                        }
                        crate::om::ReferenceKind::Tagged28 => ObjectReferenceKind::Tagged28,
                        crate::om::ReferenceKind::RecordOrdinal16 => unreachable!("filtered"),
                    },
                    value: reference.value,
                    source_offset: entry_offset + reference.offset as u64,
                })
                .collect()
        })
        .collect()
}

/// Join maximal two-token adjacent persistent-handle runs atomically.
pub fn data_block_control_handle_pairs(
    references: &[DataBlockControlReference],
) -> Vec<DataBlockControlHandlePair> {
    let mut by_block = BTreeMap::<&str, Vec<&DataBlockControlReference>>::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        by_block
            .entry(reference.data_block.as_str())
            .or_default()
            .push(reference);
    }
    let mut pairs = Vec::new();
    for (data_block, mut block_references) in by_block {
        block_references.sort_by_key(|reference| reference.source_offset);
        let mut at = 0;
        while at < block_references.len() {
            let start = at;
            while block_references
                .get(at + 1)
                .is_some_and(|next| next.source_offset == block_references[at].source_offset + 5)
            {
                at += 1;
            }
            let run = &block_references[start..=at];
            if let [first, second] = run {
                pairs.push(DataBlockControlHandlePair {
                    id: format!(
                        "nx:om-data-block-control:handle-pair#{}",
                        first.source_offset
                    ),
                    data_block: data_block.to_string(),
                    first_reference: first.id.clone(),
                    second_reference: second.id.clone(),
                    first_handle: first.value,
                    second_handle: second.value,
                    source_offset: first.source_offset,
                });
            }
            at += 1;
        }
    }
    pairs
}

/// Decode framed object references from offset-only OM data blocks.
pub fn data_block_references(container: &Container) -> Vec<DataBlockReference> {
    let mut records = BTreeMap::<(String, u32), Vec<String>>::new();
    for record in object_records(container) {
        let Some(object_id) = record.object_id else {
            continue;
        };
        records
            .entry((record.source_entry, object_id))
            .or_default()
            .push(record.id);
    }
    let mut declarations = BTreeMap::<(String, u32), Vec<String>>::new();
    for declaration in expression_declarations(container) {
        declarations
            .entry((declaration.source_entry, declaration.object_id))
            .or_default()
            .push(declaration.id);
    }
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_some())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let mut source_blocks = Vec::with_capacity(section.records.len() + 1);
            if let Some(control) = section.control {
                source_blocks.push(control);
            }
            source_blocks.extend(section.records);
            source_blocks
                .into_iter()
                .enumerate()
                .flat_map(|(block_ordinal, block)| {
                    crate::om::data_block_object_references(block.bytes)
                        .into_iter()
                        .enumerate()
                        .map(|(ordinal, reference)| {
                            let key = (entry.name.clone(), reference.object_index);
                            let unique = |candidates: Option<&Vec<String>>| {
                                let [target] = candidates?.as_slice() else {
                                    return None;
                                };
                                Some(target.clone())
                            };
                            DataBlockReference {
                                id: format!(
                                    "nx:om-data-block-references-{section_ordinal}-{block_ordinal}:reference#{ordinal}"
                                ),
                                data_block: format!(
                                    "nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"
                                ),
                                ordinal: ordinal as u32,
                                object_id: reference.object_index,
                                target_record: unique(records.get(&key)),
                                target_expression_declaration: unique(declarations.get(&key)),
                                source_offset: entry_offset
                                    + block.offset as u64
                                    + reference.offset as u64,
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .collect()
}

/// Decode complete in-range counted block-index lanes from offset-only stores.
pub fn data_block_counted_index_lanes(container: &Container) -> Vec<DataBlockCountedIndexLane> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_some())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let block_count = section.records.len() + 1;
            section
                .records
                .into_iter()
                .enumerate()
                .flat_map(|(record_ordinal, block)| {
                    let block_ordinal = record_ordinal + 1;
                    crate::om::offset_store_counted_index_lanes(block.bytes)
                        .into_iter()
                        .filter_map(|lane| {
                            let anchor_data_block = control_index_data_block(
                                section_ordinal,
                                block_count,
                                lane.anchor,
                            )?;
                            let member_data_blocks = lane
                                .members
                                .iter()
                                .map(|(value, _)| {
                                    control_index_data_block(
                                        section_ordinal,
                                        block_count,
                                        *value,
                                    )
                                })
                                .collect::<Option<Vec<_>>>()?;
                            let source_base = entry_offset + block.offset as u64;
                            Some((lane, anchor_data_block, member_data_blocks, source_base))
                        })
                        .enumerate()
                        .map(
                            |(
                                ordinal,
                                (lane, anchor_data_block, member_data_blocks, source_base),
                            )| DataBlockCountedIndexLane {
                                id: format!(
                                    "nx:om-data-block-counted-index-lanes-{section_ordinal}-{block_ordinal}:lane#{ordinal}"
                                ),
                                data_block: format!(
                                    "nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"
                                ),
                                ordinal: ordinal as u32,
                                declared_count: lane.declared_count,
                                anchor_index: lane.anchor,
                                anchor_data_block,
                                member_indices: lane
                                    .members
                                    .iter()
                                    .map(|(value, _)| *value)
                                    .collect(),
                                member_data_blocks,
                                source_offset: source_base + lane.offset as u64,
                                anchor_source_offset: source_base + lane.anchor_offset as u64,
                                member_source_offsets: lane
                                    .members
                                    .iter()
                                    .map(|(_, offset)| source_base + *offset as u64)
                                    .collect(),
                            },
                        )
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .collect()
}

/// Decode complete in-range `ABR` reference lanes from offset-store column storage.
pub fn data_block_abr_reference_lanes(container: &Container) -> Vec<DataBlockAbrReferenceLane> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(storage) = section.column_storage else {
                return Vec::new();
            };
            let Some(storage_offset) = section.records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let source_base = entry_offset + storage_offset as u64;
            let block_count = section.records.len() + 1;
            crate::om::offset_store_abr_reference_lanes(storage)
                .into_iter()
                .filter_map(|lane| {
                    let slot_data_blocks = lane
                        .slots
                        .iter()
                        .map(|(value, _)| {
                            value.map_or(Some(None), |value| {
                                control_index_data_block(section_ordinal, block_count, value)
                                    .map(Some)
                            })
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some((lane, slot_data_blocks))
                })
                .enumerate()
                .map(
                    |(ordinal, (lane, slot_data_blocks))| DataBlockAbrReferenceLane {
                        id: format!(
                            "nx:om-data-block-abr-reference-lanes-{section_ordinal}:lane#{ordinal}"
                        ),
                        section_ordinal: section_ordinal as u32,
                        ordinal: ordinal as u32,
                        slot_indices: lane.slots.iter().map(|(value, _)| *value).collect(),
                        slot_data_blocks,
                        slot_source_offsets: lane
                            .slots
                            .iter()
                            .map(|(_, offset)| source_base + *offset as u64)
                            .collect(),
                        source_entry: entry.name.clone(),
                        source_offset: source_base + lane.offset as u64,
                    },
                )
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode complete index rows from offset-store column storage.
pub fn data_block_index_rows(container: &Container) -> Vec<DataBlockIndexRow> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(storage) = section.column_storage else {
                return Vec::new();
            };
            let Some(storage_offset) = section.records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span.map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = section.records.len() + 1;
            crate::om::offset_store_index_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let data_blocks = row
                        .indices
                        .iter()
                        .map(|(index, _)| {
                            control_index_data_block(section_ordinal, block_count, *index)
                        })
                        .collect::<Option<Vec<_>>>()
                        .and_then(|blocks| blocks.try_into().ok())?;
                    let opening = column_storage_block_at(
                        section_ordinal,
                        &section.records,
                        storage_offset + row.offset,
                    )?;
                    Some((row, data_blocks, opening))
                })
                .enumerate()
                .map(|(ordinal, (row, data_blocks, opening))| DataBlockIndexRow {
                    id: format!("nx:om-data-block-index-rows-{section_ordinal}:row#{ordinal}"),
                    section_ordinal: section_ordinal as u32,
                    ordinal: ordinal as u32,
                    first_index: row.first_index,
                    flag: row.flag,
                    indices: row.indices.map(|(index, _)| index),
                    data_blocks,
                    source_entry: entry.name.clone(),
                    opening_data_block: opening.0,
                    opening_block_offset: opening.1,
                    source_offset: source_base + row.offset as u64,
                    first_index_source_offset: source_base + row.first_index_offset as u64,
                    index_source_offsets: row
                        .indices
                        .map(|(_, offset)| source_base + offset as u64),
                })
                .collect()
        })
        .collect()
}

/// Decode complete in-range linked index rows from column storage.
pub fn data_block_linked_index_rows(container: &Container) -> Vec<DataBlockLinkedIndexRow> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(storage) = section.column_storage else {
                return Vec::new();
            };
            let Some(storage_offset) = section.records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span.map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = section.records.len() + 1;
            crate::om::offset_store_linked_index_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let values = std::iter::once(row.target_index.0)
                        .chain(row.indices.iter().map(|(index, _)| *index));
                    let data_blocks = values
                        .map(|index| control_index_data_block(section_ordinal, block_count, index))
                        .collect::<Option<Vec<_>>>()
                        .and_then(|blocks| blocks.try_into().ok())?;
                    let opening = column_storage_block_at(
                        section_ordinal,
                        &section.records,
                        storage_offset + row.offset,
                    )?;
                    Some((row, data_blocks, opening))
                })
                .enumerate()
                .map(
                    |(ordinal, (row, data_blocks, opening))| DataBlockLinkedIndexRow {
                        id: format!(
                            "nx:om-data-block-linked-index-rows-{section_ordinal}:row#{ordinal}"
                        ),
                        section_ordinal: section_ordinal as u32,
                        ordinal: ordinal as u32,
                        first_index: row.first_index.0,
                        discriminator: row.discriminator,
                        target_index: row.target_index.0,
                        indices: row.indices.map(|(index, _)| index),
                        data_blocks,
                        flag: row.flag,
                        mode: row.mode,
                        source_entry: entry.name.clone(),
                        opening_data_block: opening.0,
                        opening_block_offset: opening.1,
                        source_offset: source_base + row.offset as u64,
                        first_index_source_offset: source_base + row.first_index.1 as u64,
                        target_index_source_offset: source_base + row.target_index.1 as u64,
                        index_source_offsets: row
                            .indices
                            .map(|(_, offset)| source_base + offset as u64),
                    },
                )
                .collect()
        })
        .collect()
}

/// Decode complete in-range target-index rows from column storage.
pub fn data_block_target_index_rows(container: &Container) -> Vec<DataBlockTargetIndexRow> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(storage) = section.column_storage else {
                return Vec::new();
            };
            let Some(storage_offset) = section.records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span.map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = section.records.len() + 1;
            crate::om::offset_store_target_index_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let values = std::iter::once(row.target_index.0)
                        .chain(row.indices.iter().map(|(index, _)| *index));
                    let data_blocks = values
                        .map(|index| control_index_data_block(section_ordinal, block_count, index))
                        .collect::<Option<Vec<_>>>()
                        .and_then(|blocks| blocks.try_into().ok())?;
                    let opening = column_storage_block_at(
                        section_ordinal,
                        &section.records,
                        storage_offset + row.offset,
                    )?;
                    Some((row, data_blocks, opening))
                })
                .enumerate()
                .map(
                    |(ordinal, (row, data_blocks, opening))| DataBlockTargetIndexRow {
                        id: format!(
                            "nx:om-data-block-target-index-rows-{section_ordinal}:row#{ordinal}"
                        ),
                        section_ordinal: section_ordinal as u32,
                        ordinal: ordinal as u32,
                        target_index: row.target_index.0,
                        indices: row.indices.map(|(index, _)| index),
                        data_blocks,
                        mode: row.mode,
                        source_entry: entry.name.clone(),
                        opening_data_block: opening.0,
                        opening_block_offset: opening.1,
                        source_offset: source_base + row.offset as u64,
                        target_index_source_offset: source_base + row.target_index.1 as u64,
                        index_source_offsets: row
                            .indices
                            .map(|(_, offset)| source_base + offset as u64),
                    },
                )
                .collect()
        })
        .collect()
}

/// Resolve complete composite column-index tables atomically by section.
pub fn data_block_column_index_tables(
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
) -> Vec<DataBlockColumnIndexTable> {
    let mut linked_by_section = BTreeMap::<u32, Vec<&DataBlockLinkedIndexRow>>::new();
    for row in linked_rows {
        linked_by_section
            .entry(row.section_ordinal)
            .or_default()
            .push(row);
    }
    let mut targets_by_section = BTreeMap::<u32, Vec<&DataBlockTargetIndexRow>>::new();
    for row in target_rows {
        targets_by_section
            .entry(row.section_ordinal)
            .or_default()
            .push(row);
    }
    linked_by_section
        .into_iter()
        .filter_map(|(section_ordinal, linked)| {
            let targets = targets_by_section.remove(&section_ordinal)?;
            let (opening, suffix) = linked.split_first()?;
            let (last_target, target_prefix) = targets.split_last()?;
            if opening.mode != 7
                || suffix.is_empty()
                || suffix.iter().any(|row| row.mode != 4)
                || last_target.mode != 4
                || target_prefix.iter().any(|row| row.mode != 7)
            {
                return None;
            }
            let ordered = std::iter::once((opening.target_index, opening.source_offset))
                .chain(
                    targets
                        .iter()
                        .map(|row| (row.target_index, row.source_offset)),
                )
                .chain(
                    suffix
                        .iter()
                        .map(|row| (row.target_index, row.source_offset)),
                )
                .collect::<Vec<_>>();
            if ordered
                .windows(2)
                .any(|pair| pair[0].0.checked_sub(1) != Some(pair[1].0) || pair[0].1 >= pair[1].1)
                || linked
                    .iter()
                    .any(|row| row.source_entry != opening.source_entry)
                || targets
                    .iter()
                    .any(|row| row.source_entry != opening.source_entry)
            {
                return None;
            }
            Some(DataBlockColumnIndexTable {
                id: format!("nx:om-data-block-column-index-tables:table#{section_ordinal}"),
                section_ordinal,
                opening_linked_row: opening.id.clone(),
                target_rows: targets.iter().map(|row| row.id.clone()).collect(),
                linked_rows: suffix.iter().map(|row| row.id.clone()).collect(),
                first_target_index: opening.target_index,
                last_target_index: ordered.last().expect("nonempty column table").0,
                source_entry: opening.source_entry.clone(),
                source_offset: opening.source_offset,
            })
        })
        .collect()
}

/// Decode one product/version header from each indexed NX OM store.
pub fn store_headers(container: &Container) -> Vec<StoreHeader> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .filter_map(|(section_ordinal, (entry, section))| {
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .control
                .iter()
                .chain(section.records.iter())
                .find_map(|record| {
                    crate::om::store_version(record.bytes, record.offset).map(|version| {
                        StoreHeader {
                            id: format!("nx:om-store-headers:store#{section_ordinal}"),
                            section_ordinal: section_ordinal as u32,
                            object_id: record.object_id,
                            version: version.value.to_string(),
                            source_entry: entry.name.clone(),
                            source_offset: entry_offset + version.offset as u64,
                        }
                    })
                })
        })
        .collect()
}

/// Decode self-framed printable values from bounded NX OM records.
pub fn string_values(container: &Container) -> Vec<StringValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_none())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .string_values()
                .into_iter()
                .map(move |(record_ordinal, value_ordinal, object_id, value)| {
                    let record =
                        format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                    StringValue {
                        id: format!(
                            "nx:om-string-values-{section_ordinal}-{record_ordinal}:value#{}",
                            value.offset
                        ),
                        record,
                        object_id,
                        ordinal: value_ordinal as u32,
                        value: value.value.to_string(),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + value.offset as u64,
                    }
                })
                .collect()
        })
        .collect()
}

/// Decode ordered tagged references from bounded NX OM records.
pub fn object_references(container: &Container) -> Vec<ObjectReference> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_none())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .references()
                .into_iter()
                .map(
                    move |(record_ordinal, reference_ordinal, object_id, reference)| {
                        let record = format!(
                            "nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"
                        );
                        ObjectReference {
                            id: format!(
                                "nx:om-references-{section_ordinal}-{record_ordinal}:reference#{}",
                                reference.offset
                            ),
                            record,
                            object_id,
                            ordinal: reference_ordinal as u32,
                            kind: match reference.kind {
                                crate::om::ReferenceKind::PersistentHandle => {
                                    ObjectReferenceKind::PersistentHandle
                                }
                                crate::om::ReferenceKind::Tagged28 => ObjectReferenceKind::Tagged28,
                                crate::om::ReferenceKind::RecordOrdinal16 => {
                                    ObjectReferenceKind::RecordOrdinal16
                                }
                            },
                            value: reference.value,
                            target_record: (reference.kind
                                == crate::om::ReferenceKind::RecordOrdinal16)
                                .then(|| {
                                    format!(
                                        "nx:om-record-directory-{section_ordinal}:entry#{}",
                                        reference.value
                                    )
                                }),
                            source_entry: entry.name.clone(),
                            source_offset: entry_offset + reference.offset as u64,
                        }
                    },
                )
                .collect()
        })
        .collect()
}

/// Group persistent-handle occurrences into cross-record identities.
pub fn persistent_handles(
    references: &[ObjectReference],
    control_references: &[DataBlockControlReference],
    external: &[ExternalReferenceRecord],
    external_tail_pairs: &[ExternalReferenceTailReferencePair],
) -> Vec<PersistentHandle> {
    #[derive(Default)]
    struct Group {
        records: Vec<String>,
        occurrence_count: u32,
        external_records: Vec<String>,
        data_blocks: Vec<String>,
        external_occurrence_count: u32,
    }

    let mut groups = BTreeMap::<u32, Group>::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        let group = groups.entry(reference.value).or_default();
        group.occurrence_count += 1;
        if group.records.last() != Some(&reference.record)
            && !group.records.contains(&reference.record)
        {
            group.records.push(reference.record.clone());
        }
    }
    for reference in control_references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        let group = groups.entry(reference.value).or_default();
        group.occurrence_count += 1;
        if !group.data_blocks.contains(&reference.data_block) {
            group.data_blocks.push(reference.data_block.clone());
        }
    }
    for record in external {
        for handle in &record.handles {
            let group = groups.entry(*handle).or_default();
            group.external_occurrence_count += 1;
            if !group.external_records.contains(&record.id) {
                group.external_records.push(record.id.clone());
            }
        }
        if record.closing_duplicate {
            let Some(handle) = record.handles.last() else {
                continue;
            };
            groups.entry(*handle).or_default().external_occurrence_count += 1;
        }
    }
    for pair in external_tail_pairs {
        let group = groups.entry(pair.persistent_handle).or_default();
        group.external_occurrence_count += 1;
        if !group.external_records.contains(&pair.handle_set_record) {
            group.external_records.push(pair.handle_set_record.clone());
        }
    }
    groups
        .into_iter()
        .map(|(value, group)| PersistentHandle {
            id: format!("nx:om-persistent-handles:handle#{value:08x}"),
            value,
            records: group.records,
            occurrence_count: group.occurrence_count,
            data_blocks: group.data_blocks,
            external_records: group.external_records,
            external_occurrence_count: group.external_occurrence_count,
        })
        .collect()
}

/// Decode named parameter declarations from expression-class OM records.
pub fn expression_declarations(container: &Container) -> Vec<ExpressionDeclaration> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if !section
                .types
                .iter()
                .any(|definition| definition.name == "UGS::EXP_expression")
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .records
                .into_iter()
                .enumerate()
                .filter_map(|(record_ordinal, record)| {
                    let object_id = record.object_id?;
                    let declaration = crate::om::expression_declaration_name(record.bytes)?;
                    let record_id =
                        format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                    Some(ExpressionDeclaration {
                        id: format!(
                            "nx:om-expression-declarations-{section_ordinal}:declaration#{record_ordinal}"
                        ),
                        object_id,
                        record: record_id,
                        name: declaration.value.to_string(),
                        parameter_index: declaration.parameter_index,
                        qualifier: declaration.qualifier.map(str::to_string),
                        literal: declaration.literal.map(str::to_string),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset
                            + record.offset as u64
                            + declaration.offset as u64,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode explicit numeric expressions from all indexed OM sections.
pub fn expressions(container: &Container) -> Vec<Expression> {
    let declarations = expression_declarations(container);
    let mut declarations_by_name = BTreeMap::<(&str, &str), Vec<&ExpressionDeclaration>>::new();
    for declaration in &declarations {
        declarations_by_name
            .entry((declaration.source_entry.as_str(), declaration.name.as_str()))
            .or_default()
            .push(declaration);
    }
    let mut indexed = BTreeMap::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        for (record_ordinal, expression) in section.numeric_expression_records() {
            let Some(object_id) = expression.object_id else {
                continue;
            };
            indexed.insert(
                (entry.name.clone(), expression.offset),
                (
                    Some(object_id),
                    format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"),
                ),
            );
        }
    }
    let mut expressions = Vec::new();
    for (entry_index, entry) in container.entries.iter().enumerate() {
        let Some((entry_offset, size)) = entry.file_span else {
            continue;
        };
        let (Ok(offset), Ok(size)) = (usize::try_from(entry_offset), usize::try_from(size)) else {
            continue;
        };
        let Some(payload) = container.data.get(offset..offset.saturating_add(size)) else {
            continue;
        };
        for expression in crate::om::numeric_expressions(payload) {
            let Some(table_offset) = payload[..expression.offset]
                .windows(b"hostglobalvariables".len())
                .rposition(|window| window == b"hostglobalvariables")
            else {
                continue;
            };
            let indexed_record = indexed
                .get(&(entry.name.clone(), expression.offset))
                .cloned();
            let declaration = declarations_by_name
                .get(&(entry.name.as_str(), expression.name))
                .and_then(|candidates| {
                    let same_record_arena = |first: &str, second: &str| {
                        first.split_once(":entry#").map(|pair| pair.0)
                            == second.split_once(":entry#").map(|pair| pair.0)
                    };
                    let candidates = candidates
                        .iter()
                        .copied()
                        .filter(|declaration| {
                            indexed_record.as_ref().is_none_or(|(_, record)| {
                                same_record_arena(&declaration.record, record)
                            })
                        })
                        .collect::<Vec<_>>();
                    let [declaration] = candidates.as_slice() else {
                        return None;
                    };
                    Some(declaration.id.clone())
                });
            expressions.push(Expression {
                id: format!("nx:om-entry-{entry_index}:expression#{}", expression.offset),
                object_id: indexed_record
                    .as_ref()
                    .and_then(|(object_id, _)| *object_id),
                record: indexed_record.map(|(_, record)| record),
                declaration,
                name: expression.name.to_string(),
                parameter_index: expression.parameter_index,
                qualifier: expression.qualifier.map(str::to_string),
                unit: match expression.unit {
                    crate::om::ExpressionUnit::Millimeter => ExpressionUnit::Millimeter,
                    crate::om::ExpressionUnit::Degree => ExpressionUnit::Degree,
                },
                expression: expression.expression.to_string(),
                value: expression.value,
                source_entry: entry.name.clone(),
                source_table: format!("nx:om-entry-{entry_index}:expression-table#{table_offset}"),
                source_offset: entry_offset + expression.offset as u64,
            });
        }
    }
    evaluate_expression_graphs(&mut expressions);
    expressions
}

fn expression_scope(expression: &Expression) -> &str {
    if expression.source_table.is_empty() {
        &expression.source_entry
    } else {
        &expression.source_table
    }
}

pub(crate) fn evaluate_expression_graphs(expressions: &mut [Expression]) {
    let mut name_counts = BTreeMap::<(String, String), usize>::new();
    for expression in expressions.iter() {
        *name_counts
            .entry((
                expression_scope(expression).to_string(),
                expression.name.clone(),
            ))
            .or_default() += 1;
    }
    let mut values = BTreeMap::<(String, String), (ExpressionUnit, f64)>::new();
    for expression in expressions.iter_mut() {
        let key = (
            expression_scope(expression).to_string(),
            expression.name.clone(),
        );
        if name_counts.get(&key) != Some(&1) {
            expression.value = None;
            continue;
        }
        if let Some(value) = expression.value {
            values.insert(key, (expression.unit, value));
        }
    }

    loop {
        let mut changed = false;
        for expression in expressions
            .iter_mut()
            .filter(|expression| expression.value.is_none())
        {
            let expression_key = (
                expression_scope(expression).to_string(),
                expression.name.clone(),
            );
            if name_counts.get(&expression_key) != Some(&1) {
                continue;
            }
            let mut substituted = String::with_capacity(expression.expression.len());
            let bytes = expression.expression.as_bytes();
            let mut at = 0usize;
            let mut complete = true;
            while at < bytes.len() {
                if let Some(end) = expression_parameter_reference_end(bytes, at) {
                    let start = at;
                    at = end;
                    let name = &expression.expression[start..at];
                    let key = (expression_scope(expression).to_string(), name.to_string());
                    let Some((unit, value)) = values.get(&key).copied() else {
                        complete = false;
                        break;
                    };
                    if name_counts.get(&key) != Some(&1) || expression.unit != unit {
                        complete = false;
                        break;
                    }
                    substituted.push('(');
                    substituted.push_str(&value.to_string());
                    substituted.push(')');
                    continue;
                }
                substituted.push(char::from(bytes[at]));
                at += 1;
            }
            if complete {
                if let Some(value) = crate::om::evaluate_constant_expression(&substituted) {
                    expression.value = Some(value);
                    values.insert(expression_key.clone(), (expression.unit, value));
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}
