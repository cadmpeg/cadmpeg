// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/nx.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

/// Tag constants from the table inventory.
pub(crate) mod token {
    /// `BODY` (`12`). Spec §4.1.
    pub(crate) const BODY: u8 = 12;
    /// `SHELL` (`13`). Spec §4.1.
    pub(crate) const SHELL: u8 = 13;
    /// `FACE` (`14`). Spec §4.1.
    pub(crate) const FACE: u8 = 14;
    /// `LOOP` (`15`). Spec §4.1.
    pub(crate) const LOOP: u8 = 15;
    /// `EDGE` (`16`). Spec §4.1.
    pub(crate) const EDGE: u8 = 16;
    /// `FIN` (`17`). Spec §4.1.
    pub(crate) const FIN: u8 = 17;
    /// `VERTEX` (`18`). Spec §4.1.
    pub(crate) const VERTEX: u8 = 18;
    /// `REGION` (`19`). Spec §4.1.
    pub(crate) const REGION: u8 = 19;
    /// `POINT` (`29`). Spec §4.1.
    pub(crate) const POINT: u8 = 29;
    /// `LINE` (`30`). Spec §4.1.
    pub(crate) const LINE: u8 = 30;
    /// `CIRCLE` (`31`). Spec §4.1.
    pub(crate) const CIRCLE: u8 = 31;
    /// `ELLIPSE` (`32`). Spec §4.1.
    pub(crate) const ELLIPSE: u8 = 32;
    /// `PLANE` (`50`). Spec §4.1.
    pub(crate) const PLANE: u8 = 50;
    /// `CYLINDER` (`51`). Spec §4.1.
    pub(crate) const CYLINDER: u8 = 51;
    /// `CONE` (`52`). Spec §4.1.
    pub(crate) const CONE: u8 = 52;
    /// `SPHERE` (`53`). Spec §4.1.
    pub(crate) const SPHERE: u8 = 53;
    /// `TORUS` (`54`). Spec §4.1.
    pub(crate) const TORUS: u8 = 54;
    /// `BLEND_SURF` (`56`). Spec §4.1.
    pub(crate) const BLEND_SURF: u8 = 56;
    /// `OFFSET_SURF` (`60`). Spec §4.1.
    pub(crate) const OFFSET_SURF: u8 = 60;
    /// `B_SURFACE` (`124`). Spec §4.1.
    pub(crate) const B_SURFACE: u8 = 124;
    /// `TRIMMED_CURVE` (`133`). Spec §4.1.
    pub(crate) const TRIMMED_CURVE: u8 = 133;
    /// `B_CURVE` (`134`). Spec §4.1.
    pub(crate) const B_CURVE: u8 = 134;
    /// `SP_CURVE` (`137`). Spec §4.1.
    pub(crate) const SP_CURVE: u8 = 137;
}

/// Byte offsets for the `splmsstr_header` record.
///
/// Spec §2. Record length 31 B.
///
/// ```text
/// Fixed prefix through the `HEADER` marker. The spec's byte map labels 0x1f as the start of the directory entries; the §2 prose and the parser both place `entry_count:u32 LE` there with the entries at 0x23. Recorded in the pull request.
/// ```
pub(crate) mod splmsstr_header {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 31;
    /// Offset of `magic` (`bytes[8]`). Spec §2.
    pub(crate) const MAGIC: usize = 0;
    /// Stated value of `magic` (`bytes[8]`). Spec §2.
    pub(crate) const MAGIC_VALUE: [u8; 8] = *b"SPLMSSTR";
    /// Offset of `version_tag` (`u8`). Spec §2.
    pub(crate) const VERSION_TAG: usize = 8;
    /// Offset of `file_tag` (`u24`, little-endian). Spec §2.
    pub(crate) const FILE_TAG: usize = 9;
    /// Offset of `zero_word` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_WORD: usize = 12;
    /// Offset of `zero_byte` (`u8`). Spec §2.
    pub(crate) const ZERO_BYTE: usize = 16;
    /// Offset of `footer_offset` (`u48`). Spec §2.
    pub(crate) const FOOTER_OFFSET: usize = 17;
    /// Offset of `header_marker` (`bytes[6]`). Spec §2.
    pub(crate) const HEADER_MARKER: usize = 25;
    /// Stated value of `header_marker` (`bytes[6]`). Spec §2.
    pub(crate) const HEADER_MARKER_VALUE: [u8; 6] = *b"HEADER";
}

/// Byte offsets for the `directory_entry` record.
///
/// Spec §2. Record length 4 B.
///
/// ```text
/// Only the leading count is at a fixed offset; `path[name_len]` and the 16-byte payload follow it. The path begins `/Root` and has length 6 through 128.
/// ```
pub(crate) mod directory_entry {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 4;
    /// Offset of `name_len` (`u32`, little-endian). Spec §2.
    pub(crate) const NAME_LEN: usize = 0;
}

/// Byte offsets for the `directory_file_payload` record.
///
/// Spec §2. Record length 16 B.
///
/// ```text
/// The 16-byte payload of a directory entry when it names a file. Other payloads remain exact opaque bytes.
/// ```
pub(crate) mod directory_file_payload {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 16;
    /// Offset of `file_offset` (`u64`, little-endian). Spec §2.
    pub(crate) const FILE_OFFSET: usize = 0;
    /// Offset of `size` (`u64`, little-endian). Spec §2.
    pub(crate) const SIZE: usize = 8;
}

/// Byte offsets for the `legacy_ugii_payload_prefix` record.
///
/// Spec §2.4. Record length 9 B.
///
/// ```text
/// The CFB directory path identifies the NX wrapper; the CFB signature alone is not sufficient.
/// ```
pub(crate) mod legacy_ugii_payload_prefix {
    /// Record length in bytes. Spec §2.4.
    pub(crate) const LEN: usize = 9;
    /// Offset of `marker` (`bytes[2]`). Spec §2.4.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `product` (`bytes[4]`). Spec §2.4.
    pub(crate) const PRODUCT: usize = 2;
    /// Offset of `padding` (`bytes[2]`). Spec §2.4.
    pub(crate) const PADDING: usize = 6;
    /// Offset of `version` (`u8`). Spec §2.4.
    pub(crate) const VERSION: usize = 8;
}

/// Byte offsets for the `ug_part_segment_index_row` record.
///
/// Spec §2. Record length 12 B.
///
/// ```text
/// Row ordinal 1 has `type_code = 1`, `subtype_code = 1`, and a `value` equal to the payload-relative byte offset immediately after the index.
/// ```
pub(crate) mod ug_part_segment_index_row {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 12;
    /// Offset of `type_code` (`u32`, little-endian). Spec §2.
    pub(crate) const TYPE_CODE: usize = 0;
    /// Offset of `subtype_code` (`u32`, little-endian). Spec §2.
    pub(crate) const SUBTYPE_CODE: usize = 4;
    /// Offset of `value` (`u32`, little-endian). Spec §2.
    pub(crate) const VALUE: usize = 8;
}

/// Byte offsets for the `fastload_structure_envelope` record.
///
/// Spec §2.3. Record length 12 B.
///
/// ```text
/// `payload_len + 12` equals the bounded directory-entry size. The payload begins `OM 01 01`.
/// ```
pub(crate) mod fastload_structure_envelope {
    /// Record length in bytes. Spec §2.3.
    pub(crate) const LEN: usize = 12;
    /// Offset of `signature` (`bytes[4]`). Spec §2.3.
    pub(crate) const SIGNATURE: usize = 0;
    /// Offset of `zero_word` (`bytes[4]`). Spec §2.3.
    pub(crate) const ZERO_WORD: usize = 4;
    /// Offset of `payload_len` (`u32`, big-endian). Spec §2.3.
    pub(crate) const PAYLOAD_LEN: usize = 8;
}

/// Byte offsets for the `om_section_header` record.
///
/// Spec §7.1. Record length 14 B.
///
/// ```text
/// Signature-relative. `section_end = signature_offset + 16 + payload_size`, so bytes +14..+16 belong to the header but the spec names no field for them.
/// ```
pub(crate) mod om_section_header {
    /// Record length in bytes. Spec §7.1.
    pub(crate) const LEN: usize = 14;
    /// Offset of `signature` (`bytes[4]`). Spec §7.1.
    pub(crate) const SIGNATURE: usize = 0;
    /// Offset of `payload_size` (`u32`, big-endian). Spec §7.1.
    pub(crate) const PAYLOAD_SIZE: usize = 8;
    /// Offset of `om_marker` (`bytes[2]`). Spec §7.1.
    pub(crate) const OM_MARKER: usize = 12;
}

/// Byte offsets for the `jt_document_header` record.
///
/// Spec §2.3. Record length 105 B.
///
/// ```text
/// Byte order is zero and the reserved word is zero. Field offsets are derived by laying the spec's ordered field list out from the header start; the total 105 is the derived sum.
/// ```
pub(crate) mod jt_document_header {
    /// Record length in bytes. Spec §2.3.
    pub(crate) const LEN: usize = 105;
    /// Offset of `version_field` (`bytes[80]`). Spec §2.3.
    pub(crate) const VERSION_FIELD: usize = 0;
    /// Offset of `byte_order` (`u8`). Spec §2.3.
    pub(crate) const BYTE_ORDER: usize = 80;
    /// Offset of `reserved` (`u32`, little-endian). Spec §2.3.
    pub(crate) const RESERVED: usize = 81;
    /// Offset of `toc_offset` (`u32`, little-endian). Spec §2.3.
    pub(crate) const TOC_OFFSET: usize = 85;
    /// Offset of `lsg_segment_id` (`bytes[16]`). Spec §2.3.
    pub(crate) const LSG_SEGMENT_ID: usize = 89;
}

/// Byte offsets for the `jt_toc_entry` record.
///
/// Spec §2.3. Record length 28 B.
pub(crate) mod jt_toc_entry {
    /// Record length in bytes. Spec §2.3.
    pub(crate) const LEN: usize = 28;
    /// Offset of `segment_id` (`bytes[16]`). Spec §2.3.
    pub(crate) const SEGMENT_ID: usize = 0;
    /// Offset of `segment_offset` (`u32`, little-endian). Spec §2.3.
    pub(crate) const SEGMENT_OFFSET: usize = 16;
    /// Offset of `segment_byte_len` (`u32`, little-endian). Spec §2.3.
    pub(crate) const SEGMENT_BYTE_LEN: usize = 20;
    /// Offset of `attributes` (`bytes[4]`). Spec §2.3.
    pub(crate) const ATTRIBUTES: usize = 24;
}

/// Byte offsets for the `jt_shape_lod_element_header` record.
///
/// Spec §2.3. Record length 25 B.
///
/// ```text
/// `element_byte_len` counts every byte after its own word, so `body` has length `element_byte_len - 21`.
/// ```
pub(crate) mod jt_shape_lod_element_header {
    /// Record length in bytes. Spec §2.3.
    pub(crate) const LEN: usize = 25;
    /// Offset of `element_byte_len` (`u32`, little-endian). Spec §2.3.
    pub(crate) const ELEMENT_BYTE_LEN: usize = 0;
    /// Offset of `object_type_id` (`bytes[16]`). Spec §2.3.
    pub(crate) const OBJECT_TYPE_ID: usize = 4;
    /// Offset of `object_base_type` (`u8`). Spec §2.3.
    pub(crate) const OBJECT_BASE_TYPE: usize = 20;
    /// Offset of `object_id` (`u32`, little-endian). Spec §2.3.
    pub(crate) const OBJECT_ID: usize = 21;
}

/// Byte offsets for the `jt_tristrip_shape_node_family_data` record.
///
/// Spec §2.3. Record length 100 B.
///
/// ```text
/// Offsets are derived by laying the spec's ordered field list out from the block start; the stated 100-byte total for vertex version 1 confirms the arithmetic. Vertex version 2 appends `version_2_vertex_bindings:u64 LE` and occupies 108 bytes.
/// ```
pub(crate) mod jt_tristrip_shape_node_family_data {
    /// Record length in bytes. Spec §2.3.
    pub(crate) const LEN: usize = 100;
    /// Offset of `shape_version` (`u16`, little-endian). Spec §2.3.
    pub(crate) const SHAPE_VERSION: usize = 0;
    /// Offset of `reserved_bounds` (`f32[6]`, little-endian). Spec §2.3.
    pub(crate) const RESERVED_BOUNDS: usize = 2;
    /// Offset of `untransformed_bounds` (`f32[6]`, little-endian). Spec §2.3.
    pub(crate) const UNTRANSFORMED_BOUNDS: usize = 26;
    /// Offset of `area` (`f32`, little-endian). Spec §2.3.
    pub(crate) const AREA: usize = 50;
    /// Offset of `vertex_count_range` (`i32[2]`, little-endian). Spec §2.3.
    pub(crate) const VERTEX_COUNT_RANGE: usize = 54;
    /// Offset of `node_count_range` (`i32[2]`, little-endian). Spec §2.3.
    pub(crate) const NODE_COUNT_RANGE: usize = 62;
    /// Offset of `polygon_count_range` (`i32[2]`, little-endian). Spec §2.3.
    pub(crate) const POLYGON_COUNT_RANGE: usize = 70;
    /// Offset of `memory_byte_len` (`u32`, little-endian). Spec §2.3.
    pub(crate) const MEMORY_BYTE_LEN: usize = 78;
    /// Offset of `compression_level` (`f32`, little-endian). Spec §2.3.
    pub(crate) const COMPRESSION_LEVEL: usize = 82;
    /// Offset of `vertex_version` (`u16`, little-endian). Spec §2.3.
    pub(crate) const VERTEX_VERSION: usize = 86;
    /// Offset of `vertex_bindings` (`u64`, little-endian). Spec §2.3.
    pub(crate) const VERTEX_BINDINGS: usize = 88;
    /// Offset of `vertex_quantization_bits` (`u8`). Spec §2.3.
    pub(crate) const VERTEX_QUANTIZATION_BITS: usize = 96;
    /// Offset of `normal_quantization_factor` (`u8`). Spec §2.3.
    pub(crate) const NORMAL_QUANTIZATION_FACTOR: usize = 97;
    /// Offset of `texture_quantization_bits` (`u8`). Spec §2.3.
    pub(crate) const TEXTURE_QUANTIZATION_BITS: usize = 98;
    /// Offset of `color_quantization_bits` (`u8`). Spec §2.3.
    pub(crate) const COLOR_QUANTIZATION_BITS: usize = 99;
}

/// Byte offsets for the `toggle_information_stream` record.
///
/// Spec §2.2. Record length 5 B.
///
/// ```text
/// Fixed prefix only; `count` members of `byte_len:u16 LE, value:utf8[byte_len]` follow, then a four-byte trailer. `count` covers the members and the trailer.
/// ```
pub(crate) mod toggle_information_stream {
    /// Record length in bytes. Spec §2.2.
    pub(crate) const LEN: usize = 5;
    /// Offset of `version` (`u8`). Spec §2.2.
    pub(crate) const VERSION: usize = 0;
    /// Offset of `count` (`u32`, little-endian). Spec §2.2.
    pub(crate) const COUNT: usize = 1;
}

/// Byte offsets for the `extrefstream_header` record.
///
/// Spec §2.3. Record length 20 B.
///
/// ```text
/// The spec's field list places the record region at byte 20. The parser expects the record region's leading `0x00` at byte 24 and the first directory pair at 25, leaving bytes 20..24 undescribed. Recorded in the pull request.
/// ```
pub(crate) mod extrefstream_header {
    /// Record length in bytes. Spec §2.3.
    pub(crate) const LEN: usize = 20;
    /// Offset of `magic` (`bytes[12]`). Spec §2.3.
    pub(crate) const MAGIC: usize = 0;
    /// Offset of `version` (`u32`, little-endian). Spec §2.3.
    pub(crate) const VERSION: usize = 12;
    /// Offset of `payload_size` (`u32`, little-endian). Spec §2.3.
    pub(crate) const PAYLOAD_SIZE: usize = 16;
}

/// Byte offsets for the `extrefstream_handle_set_record` record.
///
/// Spec §9.1. Record length 25 B.
///
/// ```text
/// Fixed prefix only. `count - 1` occurrences of `e0 + handle:u32 BE` follow at +25, then a closing byte equal to `count`. Note the mixed lane: `n` is big-endian while the four ID slots are little-endian.
/// ```
pub(crate) mod extrefstream_handle_set_record {
    /// Record length in bytes. Spec §9.1.
    pub(crate) const LEN: usize = 25;
    /// Offset of `lead` (`bytes[4]`). Spec §9.1.
    pub(crate) const LEAD: usize = 0;
    /// Offset of `n` (`u16`, big-endian). Spec §9.1.
    pub(crate) const N: usize = 4;
    /// Offset of `marker_a` (`u8`). Spec §9.1.
    pub(crate) const MARKER_A: usize = 6;
    /// Offset of `id_slots` (`u32[4]`, little-endian). Spec §9.1.
    pub(crate) const ID_SLOTS: usize = 7;
    /// Offset of `marker_b` (`u8`). Spec §9.1.
    pub(crate) const MARKER_B: usize = 23;
    /// Offset of `count` (`u8`). Spec §9.1.
    pub(crate) const COUNT: usize = 24;
}

/// Byte offsets for the `analytic_common_header` record.
///
/// Spec §5.1. Record length 19 B.
///
/// ```text
/// Record-relative, after shifts. Each extended reference in the five-reference common header shifts the analytic payload and record end by two bytes, and the shifts accumulate.
/// ```
pub(crate) mod analytic_common_header {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 19;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
    /// Offset of `owner` (`xmt_ref`). Spec §5.1.
    pub(crate) const OWNER: usize = 10;
    /// Offset of `next` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT: usize = 12;
    /// Offset of `previous` (`xmt_ref`). Spec §5.1.
    pub(crate) const PREVIOUS: usize = 14;
    /// Offset of `group` (`xmt_ref`). Spec §5.1.
    pub(crate) const GROUP: usize = 16;
    /// Offset of `sense` (`u8`). Spec §5.1.
    pub(crate) const SENSE: usize = 18;
}

/// Byte offsets for the `face_node` record.
///
/// Spec §5.1. Record length 39 B.
///
/// ```text
/// Record-relative, after shifts. Unannotated fields are two-byte XMT references. FACE `tolerance` decodes as the sentinel `-3.14158e13` when unset. Any fixed record may place an envelope escape byte `ff` between its type and XMT fields, shifting every logical payload offset by one.
/// ```
pub(crate) mod face_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 39;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
    /// Offset of `tolerance` (`f64`, big-endian). Spec §5.1.
    pub(crate) const TOLERANCE: usize = 10;
    /// Offset of `next_face` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT_FACE: usize = 18;
    /// Offset of `prev_face` (`xmt_ref`). Spec §5.1.
    pub(crate) const PREV_FACE: usize = 20;
    /// Offset of `loop` (`xmt_ref`). Spec §5.1.
    pub(crate) const LOOP: usize = 22;
    /// Offset of `shell` (`xmt_ref`). Spec §5.1.
    pub(crate) const SHELL: usize = 24;
    /// Offset of `surface` (`xmt_ref`). Spec §5.1.
    pub(crate) const SURFACE: usize = 26;
    /// Offset of `sense` (`u8`). Spec §5.1.
    pub(crate) const SENSE: usize = 28;
    /// Offset of `next_on_surface` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT_ON_SURFACE: usize = 29;
    /// Offset of `prev_on_surface` (`xmt_ref`). Spec §5.1.
    pub(crate) const PREV_ON_SURFACE: usize = 31;
    /// Offset of `next_front` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT_FRONT: usize = 33;
    /// Offset of `prev_front` (`xmt_ref`). Spec §5.1.
    pub(crate) const PREV_FRONT: usize = 35;
    /// Offset of `front_shell` (`xmt_ref`). Spec §5.1.
    pub(crate) const FRONT_SHELL: usize = 37;
}

/// Byte offsets for the `edge_node` record.
///
/// Spec §5.1. Record length 32 B.
pub(crate) mod edge_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 32;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
    /// Offset of `tolerance` (`f64`, big-endian). Spec §5.1.
    pub(crate) const TOLERANCE: usize = 10;
    /// Offset of `fin` (`xmt_ref`). Spec §5.1.
    pub(crate) const FIN: usize = 18;
    /// Offset of `prev_edge` (`xmt_ref`). Spec §5.1.
    pub(crate) const PREV_EDGE: usize = 20;
    /// Offset of `next_edge` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT_EDGE: usize = 22;
    /// Offset of `curve` (`xmt_ref`). Spec §5.1.
    pub(crate) const CURVE: usize = 24;
    /// Offset of `next_on_curve` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT_ON_CURVE: usize = 26;
    /// Offset of `prev_on_curve` (`xmt_ref`). Spec §5.1.
    pub(crate) const PREV_ON_CURVE: usize = 28;
    /// Offset of `owner` (`xmt_ref`). Spec §5.1.
    pub(crate) const OWNER: usize = 30;
}

/// Byte offsets for the `fin_node` record.
///
/// Spec §5.1. Record length 23 B.
///
/// ```text
/// FIN has no `node_id`, so its field block starts at +4 rather than +8.
/// ```
pub(crate) mod fin_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 23;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 4;
    /// Offset of `loop` (`xmt_ref`). Spec §5.1.
    pub(crate) const LOOP: usize = 6;
    /// Offset of `forward_fin` (`xmt_ref`). Spec §5.1.
    pub(crate) const FORWARD_FIN: usize = 8;
    /// Offset of `backward_fin` (`xmt_ref`). Spec §5.1.
    pub(crate) const BACKWARD_FIN: usize = 10;
    /// Offset of `vertex` (`xmt_ref`). Spec §5.1.
    pub(crate) const VERTEX: usize = 12;
    /// Offset of `other_fin` (`xmt_ref`). Spec §5.1.
    pub(crate) const OTHER_FIN: usize = 14;
    /// Offset of `edge` (`xmt_ref`). Spec §5.1.
    pub(crate) const EDGE: usize = 16;
    /// Offset of `curve` (`xmt_ref`). Spec §5.1.
    pub(crate) const CURVE: usize = 18;
    /// Offset of `next_at_vertex` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT_AT_VERTEX: usize = 20;
    /// Offset of `sense` (`u8`). Spec §5.1.
    pub(crate) const SENSE: usize = 22;
}

/// Byte offsets for the `vertex_node` record.
///
/// Spec §5.1. Record length 28 B.
pub(crate) mod vertex_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 28;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
    /// Offset of `fin` (`xmt_ref`). Spec §5.1.
    pub(crate) const FIN: usize = 10;
    /// Offset of `prev_vertex` (`xmt_ref`). Spec §5.1.
    pub(crate) const PREV_VERTEX: usize = 12;
    /// Offset of `next_vertex` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT_VERTEX: usize = 14;
    /// Offset of `point` (`xmt_ref`). Spec §5.1.
    pub(crate) const POINT: usize = 16;
    /// Offset of `tolerance` (`f64`, big-endian). Spec §5.1.
    pub(crate) const TOLERANCE: usize = 18;
    /// Offset of `owner` (`xmt_ref`). Spec §5.1.
    pub(crate) const OWNER: usize = 26;
}

/// Byte offsets for the `loop_node` record.
///
/// Spec §5.1. Record length 16 B.
pub(crate) mod loop_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 16;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
    /// Offset of `fin` (`xmt_ref`). Spec §5.1.
    pub(crate) const FIN: usize = 10;
    /// Offset of `face` (`xmt_ref`). Spec §5.1.
    pub(crate) const FACE: usize = 12;
    /// Offset of `next_loop` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT_LOOP: usize = 14;
}

/// Byte offsets for the `shell_node` record.
///
/// Spec §5.1. Record length 24 B.
pub(crate) mod shell_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 24;
    /// Offset of `node_id` (`u32`, big-endian). Spec §5.1.
    pub(crate) const NODE_ID: usize = 4;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
    /// Offset of `body_ref` (`xmt_ref`). Spec §5.1.
    pub(crate) const BODY_REF: usize = 10;
    /// Offset of `next_shell` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT_SHELL: usize = 12;
    /// Offset of `first_face` (`xmt_ref`). Spec §5.1.
    pub(crate) const FIRST_FACE: usize = 14;
    /// Offset of `sentinel_16` (`xmt_ref`). Spec §5.1.
    pub(crate) const SENTINEL_16: usize = 16;
    /// Offset of `sentinel_18` (`xmt_ref`). Spec §5.1.
    pub(crate) const SENTINEL_18: usize = 18;
    /// Offset of `region_ref` (`xmt_ref`). Spec §5.1.
    pub(crate) const REGION_REF: usize = 20;
    /// Offset of `face_anchor` (`xmt_ref`). Spec §5.1.
    pub(crate) const FACE_ANCHOR: usize = 22;
}

/// Byte offsets for the `point_node` record.
///
/// Spec §5.1. Record length 40 B.
pub(crate) mod point_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 40;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
    /// Offset of `owner` (`xmt_ref`). Spec §5.1.
    pub(crate) const OWNER: usize = 10;
    /// Offset of `next` (`xmt_ref`). Spec §5.1.
    pub(crate) const NEXT: usize = 12;
    /// Offset of `prev` (`xmt_ref`). Spec §5.1.
    pub(crate) const PREV: usize = 14;
    /// Offset of `xyz` (`f64[3]`, big-endian). Spec §5.1.
    pub(crate) const XYZ: usize = 16;
}

/// Byte offsets for the `line_payload` record.
///
/// Spec §6.1. Record length 67 B.
///
/// ```text
/// Payload offsets are relative to the record's type tag, after the common header (§5.1). Each point or vector is three f64 BE. The 67-byte total is the §4.1 fixed record length for type 30.
/// ```
pub(crate) mod line_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 67;
    /// Offset of `point` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const POINT: usize = 19;
    /// Offset of `direction` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const DIRECTION: usize = 43;
}

/// Byte offsets for the `circle_payload` record.
///
/// Spec §6.1. Record length 99 B.
pub(crate) mod circle_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 99;
    /// Offset of `center` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const CENTER: usize = 19;
    /// Offset of `normal` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const NORMAL: usize = 43;
    /// Offset of `x_axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const X_AXIS: usize = 67;
    /// Offset of `radius` (`f64`, big-endian). Spec §6.1.
    pub(crate) const RADIUS: usize = 91;
}

/// Byte offsets for the `ellipse_payload` record.
///
/// Spec §6.1. Record length 107 B.
pub(crate) mod ellipse_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 107;
    /// Offset of `center` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const CENTER: usize = 19;
    /// Offset of `normal` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const NORMAL: usize = 43;
    /// Offset of `x_axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const X_AXIS: usize = 67;
    /// Offset of `major` (`f64`, big-endian). Spec §6.1.
    pub(crate) const MAJOR: usize = 91;
    /// Offset of `minor` (`f64`, big-endian). Spec §6.1.
    pub(crate) const MINOR: usize = 99;
}

/// Byte offsets for the `plane_payload` record.
///
/// Spec §6.1. Record length 91 B.
pub(crate) mod plane_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 91;
    /// Offset of `origin` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const ORIGIN: usize = 19;
    /// Offset of `normal` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const NORMAL: usize = 43;
    /// Offset of `x_axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const X_AXIS: usize = 67;
}

/// Byte offsets for the `cylinder_payload` record.
///
/// Spec §6.1. Record length 99 B.
pub(crate) mod cylinder_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 99;
    /// Offset of `origin` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const ORIGIN: usize = 19;
    /// Offset of `axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const AXIS: usize = 43;
    /// Offset of `radius` (`f64`, big-endian). Spec §6.1.
    pub(crate) const RADIUS: usize = 67;
    /// Offset of `x_axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const X_AXIS: usize = 75;
}

/// Byte offsets for the `cone_payload` record.
///
/// Spec §6.1. Record length 115 B.
pub(crate) mod cone_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 115;
    /// Offset of `origin` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const ORIGIN: usize = 19;
    /// Offset of `axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const AXIS: usize = 43;
    /// Offset of `radius` (`f64`, big-endian). Spec §6.1.
    pub(crate) const RADIUS: usize = 67;
    /// Offset of `sin_half` (`f64`, big-endian). Spec §6.1.
    pub(crate) const SIN_HALF: usize = 75;
    /// Offset of `cos_half` (`f64`, big-endian). Spec §6.1.
    pub(crate) const COS_HALF: usize = 83;
    /// Offset of `x_axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const X_AXIS: usize = 91;
}

/// Byte offsets for the `sphere_payload` record.
///
/// Spec §6.1. Record length 99 B.
///
/// ```text
/// Note the slot order: the radius sits between the centre and the axis.
/// ```
pub(crate) mod sphere_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 99;
    /// Offset of `center` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const CENTER: usize = 19;
    /// Offset of `radius` (`f64`, big-endian). Spec §6.1.
    pub(crate) const RADIUS: usize = 43;
    /// Offset of `axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const AXIS: usize = 51;
    /// Offset of `x_axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const X_AXIS: usize = 75;
}

/// Byte offsets for the `torus_payload` record.
///
/// Spec §6.1. Record length 107 B.
pub(crate) mod torus_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 107;
    /// Offset of `center` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const CENTER: usize = 19;
    /// Offset of `axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const AXIS: usize = 43;
    /// Offset of `major` (`f64`, big-endian). Spec §6.1.
    pub(crate) const MAJOR: usize = 67;
    /// Offset of `minor` (`f64`, big-endian). Spec §6.1.
    pub(crate) const MINOR: usize = 75;
    /// Offset of `x_axis` (`f64[3]`, big-endian). Spec §6.1.
    pub(crate) const X_AXIS: usize = 83;
}

/// Byte offsets for the `offset_surf_payload` record.
///
/// Spec §6.1. Record length 31 B.
///
/// ```text
/// The compact partition record ends after `offset_distance`, closing the §4.1 length of 31. The status-framed deltas form continues with one finite `state_scalar:f64 BE` outside this extent.
/// ```
pub(crate) mod offset_surf_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 31;
    /// Offset of `discriminator` (`u8`). Spec §6.1.
    pub(crate) const DISCRIMINATOR: usize = 19;
    /// Offset of `true_offset` (`u8`). Spec §6.1.
    pub(crate) const TRUE_OFFSET: usize = 20;
    /// Offset of `base_surface` (`xmt_ref`). Spec §6.1.
    pub(crate) const BASE_SURFACE: usize = 21;
    /// Offset of `offset_distance` (`f64`, big-endian). Spec §6.1.
    pub(crate) const OFFSET_DISTANCE: usize = 23;
}

/// Byte offsets for the `trimmed_curve_payload` record.
///
/// Spec §6.4. Record length 85 B.
///
/// ```text
/// A large-index basis-curve reference shifts every later field by two bytes.
/// ```
pub(crate) mod trimmed_curve_payload {
    /// Record length in bytes. Spec §6.4.
    pub(crate) const LEN: usize = 85;
    /// Offset of `basis_curve` (`xmt_ref`). Spec §6.4.
    pub(crate) const BASIS_CURVE: usize = 19;
    /// Offset of `point_1` (`f64[3]`, big-endian). Spec §6.4.
    pub(crate) const POINT_1: usize = 21;
    /// Offset of `point_2` (`f64[3]`, big-endian). Spec §6.4.
    pub(crate) const POINT_2: usize = 45;
    /// Offset of `parm_1` (`f64`, big-endian). Spec §6.4.
    pub(crate) const PARM_1: usize = 69;
    /// Offset of `parm_2` (`f64`, big-endian). Spec §6.4.
    pub(crate) const PARM_2: usize = 77;
}

/// Byte offsets for the `sp_curve_payload` record.
///
/// Spec §6.4. Record length 33 B.
pub(crate) mod sp_curve_payload {
    /// Record length in bytes. Spec §6.4.
    pub(crate) const LEN: usize = 33;
    /// Offset of `surface` (`xmt_ref`). Spec §6.4.
    pub(crate) const SURFACE: usize = 19;
    /// Offset of `b_curve` (`xmt_ref`). Spec §6.4.
    pub(crate) const B_CURVE: usize = 21;
    /// Offset of `original` (`xmt_ref`). Spec §6.4.
    pub(crate) const ORIGINAL: usize = 23;
    /// Offset of `tolerance_to_original` (`f64`, big-endian). Spec §6.4.
    pub(crate) const TOLERANCE_TO_ORIGINAL: usize = 25;
}

/// Byte offsets for the `intersection_type_38` record.
///
/// Spec §6.3. Record length 31 B.
///
/// ```text
/// §4.1's fixed-record table has no row for type 38; the 31-byte total here is the parser's constant, which the six stated reference offsets close exactly. Recorded in the pull request.
/// ```
pub(crate) mod intersection_type_38 {
    /// Record length in bytes. Spec §6.3.
    pub(crate) const LEN: usize = 31;
    /// Offset of `ref0_primary_support` (`xmt_ref`). Spec §6.3.
    pub(crate) const REF0_PRIMARY_SUPPORT: usize = 19;
    /// Offset of `ref1_second_support_bridge` (`xmt_ref`). Spec §6.3.
    pub(crate) const REF1_SECOND_SUPPORT_BRIDGE: usize = 21;
    /// Offset of `ref2_chart` (`xmt_ref`). Spec §6.3.
    pub(crate) const REF2_CHART: usize = 23;
    /// Offset of `ref3_term_start` (`xmt_ref`). Spec §6.3.
    pub(crate) const REF3_TERM_START: usize = 25;
    /// Offset of `ref4_term_end` (`xmt_ref`). Spec §6.3.
    pub(crate) const REF4_TERM_END: usize = 27;
    /// Offset of `ref5_values_array` (`xmt_ref`). Spec §6.3.
    pub(crate) const REF5_VALUES_ARRAY: usize = 29;
}

/// Byte offsets for the `chart_s_preamble` record.
///
/// Spec §6.3. Record length 52 B.
///
/// ```text
/// Offsets are relative to `pre`, the end of the `count` and `xmt` fields. The Hvec block always starts at `pre+52`. Field offsets are derived by laying the spec's ordered field list out from `pre`; the stated `pre+52` block start confirms the arithmetic.
/// ```
pub(crate) mod chart_s_preamble {
    /// Record length in bytes. Spec §6.3.
    pub(crate) const LEN: usize = 52;
    /// Offset of `base_parameter` (`f64`, big-endian). Spec §6.3.
    pub(crate) const BASE_PARAMETER: usize = 0;
    /// Offset of `base_scale` (`f64`, big-endian). Spec §6.3.
    pub(crate) const BASE_SCALE: usize = 8;
    /// Offset of `chart_count` (`u32`, big-endian). Spec §6.3.
    pub(crate) const CHART_COUNT: usize = 16;
    /// Offset of `chordal_error` (`f64`, big-endian). Spec §6.3.
    pub(crate) const CHORDAL_ERROR: usize = 20;
    /// Offset of `angular_error` (`f64`, big-endian). Spec §6.3.
    pub(crate) const ANGULAR_ERROR: usize = 28;
    /// Offset of `parameter_error` (`f64[2]`, big-endian). Spec §6.3.
    pub(crate) const PARAMETER_ERROR: usize = 36;
}

/// Byte offsets for the `nurbs_surface_descriptor_prefix` record.
///
/// Spec §6.2. Record length 28 B.
///
/// ```text
/// Offsets are relative to the type tag after the optional envelope and large-index shift. The prefix ends at the V distinct-knot count; the later reference layout is variable-width.
/// ```
pub(crate) mod nurbs_surface_descriptor_prefix {
    /// Record length in bytes. Spec §6.2.
    pub(crate) const LEN: usize = 28;
    /// Offset of `u_periodic` (`u8`). Spec §6.2.
    pub(crate) const U_PERIODIC: usize = 4;
    /// Offset of `v_periodic` (`u8`). Spec §6.2.
    pub(crate) const V_PERIODIC: usize = 5;
    /// Offset of `u_degree` (`u16`, big-endian). Spec §6.2.
    pub(crate) const U_DEGREE: usize = 6;
    /// Offset of `v_degree` (`u16`, big-endian). Spec §6.2.
    pub(crate) const V_DEGREE: usize = 8;
    /// Offset of `u_pole_count` (`u32`, big-endian). Spec §6.2.
    pub(crate) const U_POLE_COUNT: usize = 10;
    /// Offset of `v_pole_count` (`u32`, big-endian). Spec §6.2.
    pub(crate) const V_POLE_COUNT: usize = 14;
    /// Offset of `u_knot_type` (`u8`). Spec §6.2.
    pub(crate) const U_KNOT_TYPE: usize = 18;
    /// Offset of `v_knot_type` (`u8`). Spec §6.2.
    pub(crate) const V_KNOT_TYPE: usize = 19;
    /// Offset of `u_distinct_knot_count` (`u32`, big-endian). Spec §6.2.
    pub(crate) const U_DISTINCT_KNOT_COUNT: usize = 20;
    /// Offset of `v_distinct_knot_count` (`u32`, big-endian). Spec §6.2.
    pub(crate) const V_DISTINCT_KNOT_COUNT: usize = 24;
}

/// Byte offsets for the `nurbs_curve_descriptor_prefix` record.
///
/// Spec §6.2. Record length 21 B.
///
/// ```text
/// Offsets are relative to the type tag after the optional envelope and large-index shift. The reference lane begins at +21 or +23 depending on the selected descriptor framing.
/// ```
pub(crate) mod nurbs_curve_descriptor_prefix {
    /// Record length in bytes. Spec §6.2.
    pub(crate) const LEN: usize = 21;
    /// Offset of `degree` (`u16`, big-endian). Spec §6.2.
    pub(crate) const DEGREE: usize = 4;
    /// Offset of `pole_count` (`u32`, big-endian). Spec §6.2.
    pub(crate) const POLE_COUNT: usize = 6;
    /// Offset of `dimension` (`u16`, big-endian). Spec §6.2.
    pub(crate) const DIMENSION: usize = 10;
    /// Offset of `distinct_knot_count` (`u32`, big-endian). Spec §6.2.
    pub(crate) const DISTINCT_KNOT_COUNT: usize = 12;
    /// Offset of `knot_type` (`u8`). Spec §6.2.
    pub(crate) const KNOT_TYPE: usize = 16;
    /// Offset of `periodic` (`u8`). Spec §6.2.
    pub(crate) const PERIODIC: usize = 17;
    /// Offset of `closed` (`u8`). Spec §6.2.
    pub(crate) const CLOSED: usize = 18;
    /// Offset of `rational` (`u8`). Spec §6.2.
    pub(crate) const RATIONAL: usize = 19;
    /// Offset of `curve_form` (`u8`). Spec §6.2.
    pub(crate) const CURVE_FORM: usize = 20;
}
