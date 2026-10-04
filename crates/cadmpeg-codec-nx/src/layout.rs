// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/nx.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

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
    /// Stated value of `magic` (`bytes[8]`). Spec §2.
    pub(crate) const MAGIC_VALUE: [u8; 8] = *b"SPLMSSTR";
    /// Offset of `version_tag` (`u8`). Spec §2.
    pub(crate) const VERSION_TAG: usize = 8;
    /// Offset of `file_tag` (`u24`, little-endian). Spec §2.
    pub(crate) const FILE_TAG: usize = 9;
    /// Offset of `footer_offset` (`u48`). Spec §2.
    pub(crate) const FOOTER_OFFSET: usize = 17;
    /// Offset of `header_marker` (`bytes[6]`). Spec §2.
    pub(crate) const HEADER_MARKER: usize = 25;
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
    /// Offset of `payload_len` (`u32`, big-endian). Spec §2.3.
    pub(crate) const PAYLOAD_LEN: usize = 8;
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
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
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
}

/// Byte offsets for the `edge_node` record.
///
/// Spec §5.1. Record length 32 B.
pub(crate) mod edge_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 32;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
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
}

/// Byte offsets for the `vertex_node` record.
///
/// Spec §5.1. Record length 28 B.
pub(crate) mod vertex_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 28;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
}

/// Byte offsets for the `loop_node` record.
///
/// Spec §5.1. Record length 16 B.
pub(crate) mod loop_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 16;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
}

/// Byte offsets for the `shell_node` record.
///
/// Spec §5.1. Record length 24 B.
pub(crate) mod shell_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 24;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
}

/// Byte offsets for the `point_node` record.
///
/// Spec §5.1. Record length 40 B.
pub(crate) mod point_node {
    /// Record length in bytes. Spec §5.1.
    pub(crate) const LEN: usize = 40;
    /// Offset of `attributes` (`xmt_ref`). Spec §5.1.
    pub(crate) const ATTRIBUTES: usize = 8;
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
}

/// Byte offsets for the `circle_payload` record.
///
/// Spec §6.1. Record length 99 B.
pub(crate) mod circle_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 99;
}

/// Byte offsets for the `ellipse_payload` record.
///
/// Spec §6.1. Record length 107 B.
pub(crate) mod ellipse_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 107;
}

/// Byte offsets for the `plane_payload` record.
///
/// Spec §6.1. Record length 91 B.
pub(crate) mod plane_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 91;
}

/// Byte offsets for the `cylinder_payload` record.
///
/// Spec §6.1. Record length 99 B.
pub(crate) mod cylinder_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 99;
}

/// Byte offsets for the `cone_payload` record.
///
/// Spec §6.1. Record length 115 B.
pub(crate) mod cone_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 115;
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
}

/// Byte offsets for the `torus_payload` record.
///
/// Spec §6.1. Record length 107 B.
pub(crate) mod torus_payload {
    /// Record length in bytes. Spec §6.1.
    pub(crate) const LEN: usize = 107;
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
}

/// Byte offsets for the `sp_curve_payload` record.
///
/// Spec §6.4. Record length 33 B.
pub(crate) mod sp_curve_payload {
    /// Record length in bytes. Spec §6.4.
    pub(crate) const LEN: usize = 33;
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
}

/// Byte offsets for the `nurbs_surface_descriptor_prefix` record.
///
/// Spec §6.2. Record length 28 B.
///
/// ```text
/// Offsets are relative to the type tag after the optional envelope and large-index shift. The prefix ends at the V distinct-knot count; the later reference layout is variable-width.
/// ```
pub(crate) mod nurbs_surface_descriptor_prefix {
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
}
