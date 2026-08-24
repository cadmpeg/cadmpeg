// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/sldprt.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

// Records omitted because the table declares a contradiction.
//
// - `chart_00_28` (overlap): The spec's ordered field list places the unnamed seventh f64 at body +34..+42, which overlaps the sentinel the same paragraph puts at body +36. The two are consistent only if the stated `+36`, `+44`, and `+52` are measured from the byte after `attr` (body +6) rather than from the body start; `crates/cadmpeg-codec-sldprt/src/brep/intersection.rs` reads them that way, placing the sentinels at body +42 and +50 and the point block at body +58. §4 uses body-relative offsets elsewhere (`00 1d` xyz at body +14), so the two conventions collide inside one document. The spec does not say which applies here.

/// Tag constants from the table inventory.
pub(crate) mod token {
    /// `bridge` (`00 0e`). Spec §4.
    pub(crate) const BRIDGE: [u8; 2] = [0x00, 0x0e];
    /// `loop head` (`00 0f`). Spec §4.
    pub(crate) const LOOP_HEAD: [u8; 2] = [0x00, 0x0f];
    /// `edge-use` (`00 10`). Spec §4.
    pub(crate) const EDGE_USE: [u8; 2] = [0x00, 0x10];
    /// `oriented coedge` (`00 11`). Spec §4.
    pub(crate) const ORIENTED_COEDGE: [u8; 2] = [0x00, 0x11];
    /// `vertex-use` (`00 12`). Spec §4.
    pub(crate) const VERTEX_USE: [u8; 2] = [0x00, 0x12];
    /// `world point` (`00 1d`). Spec §4.
    pub(crate) const WORLD_POINT: [u8; 2] = [0x00, 0x1d];
}

/// Byte offsets for the `feature_input_shifted_scalar_trailer` record.
///
/// Spec §2. Record length 35 B.
///
/// ```text
/// The value-only scalar's fixed trailer prefix. Variable-count feature_input_operand_cell12 records follow at +35.
/// ```
pub(crate) mod feature_input_shifted_scalar_trailer {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 35;
    /// Offset of `zero_prefix` (`bytes[3]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 0;
    /// Offset of `object_id` (`u32`, little-endian). Spec §2.
    pub(crate) const OBJECT_ID: usize = 3;
    /// Offset of `zero_object_tail` (`bytes[14]`). Spec §2.
    pub(crate) const ZERO_OBJECT_TAIL: usize = 7;
    /// Offset of `layout_marker` (`bytes[6]`). Spec §2.
    pub(crate) const LAYOUT_MARKER: usize = 21;
    /// Offset of `role` (`u8`). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `zero_tail` (`bytes[7]`). Spec §2.
    pub(crate) const ZERO_TAIL: usize = 28;
}

/// Byte offsets for the `feature_input_operand_cell12` record.
///
/// Spec §2. Record length 12 B.
///
/// ```text
/// Primary and legacy named-scalar operand cell. A lane-local class declaration can begin immediately after this cell.
/// ```
pub(crate) mod feature_input_operand_cell12 {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 12;
    /// Offset of `class_token` (`u16`, little-endian). Spec §2.
    pub(crate) const CLASS_TOKEN: usize = 0;
    /// Offset of `marker_address` (`u16`, little-endian). Spec §2.
    pub(crate) const MARKER_ADDRESS: usize = 2;
    /// Offset of `reference_sentinel` (`bytes[4]`). Spec §2.
    pub(crate) const REFERENCE_SENTINEL: usize = 4;
    /// Offset of `zero_trailer` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 8;
}

/// Byte offsets for the `outer_header` record.
///
/// Spec §1.1. Record length 8 B.
pub(crate) mod outer_header {
    /// Record length in bytes. Spec §1.1.
    pub(crate) const LEN: usize = 8;
    /// Offset of `file_id` (`u32`, endianness unstated). Spec §1.1.
    pub(crate) const FILE_ID: usize = 0;
    /// Offset of `version` (`u32`, big-endian). Spec §1.1.
    pub(crate) const VERSION: usize = 4;
}

/// Byte offsets for the `block_frame_header` record.
///
/// Spec §1.1. Record length 26 B.
///
/// ```text
/// Fixed prefix only. `preamble[pre_sz]` and `payload[comp_sz]` follow; the record extent is `block_end = marker_offset + 26 + pre_sz + comp_sz`.
/// ```
pub(crate) mod block_frame_header {
    /// Record length in bytes. Spec §1.1.
    pub(crate) const LEN: usize = 26;
    /// Offset of `marker` (`bytes[6]`). Spec §1.1.
    pub(crate) const MARKER: usize = 0;
    /// Stated value of `marker` (`bytes[6]`). Spec §1.1.
    pub(crate) const MARKER_VALUE: [u8; 6] = [0x14, 0x00, 0x06, 0x00, 0x08, 0x00];
    /// Offset of `type_id` (`u32`, little-endian). Spec §1.1.
    pub(crate) const TYPE_ID: usize = 6;
    /// Offset of `crc32` (`u32`, little-endian). Spec §1.1.
    pub(crate) const CRC32: usize = 10;
    /// Offset of `comp_sz` (`u32`, little-endian). Spec §1.1.
    pub(crate) const COMP_SZ: usize = 14;
    /// Offset of `uncomp_sz` (`u32`, little-endian). Spec §1.1.
    pub(crate) const UNCOMP_SZ: usize = 18;
    /// Offset of `pre_sz` (`u32`, little-endian). Spec §1.1.
    pub(crate) const PRE_SZ: usize = 22;
}

/// Byte offsets for the `cache_cell_header` record.
///
/// Spec §1.2. Record length 26 B.
///
/// ```text
/// Fixed prefix only; a nibble-swapped section name of `name_len` bytes follows at +26. The three size fields are redundant scalings of one logical value `L`.
/// ```
pub(crate) mod cache_cell_header {
    /// Record length in bytes. Spec §1.2.
    pub(crate) const LEN: usize = 26;
    /// Offset of `two_l` (`u32`, endianness unstated). Spec §1.2.
    pub(crate) const TWO_L: usize = 10;
    /// Offset of `half_l` (`u32`, endianness unstated). Spec §1.2.
    pub(crate) const HALF_L: usize = 14;
    /// Offset of `l` (`u32`, endianness unstated). Spec §1.2.
    pub(crate) const L: usize = 18;
    /// Offset of `name_len` (`u32`, endianness unstated). Spec §1.2.
    pub(crate) const NAME_LEN: usize = 22;
}

/// Byte offsets for the `tail_directory_entry` record.
///
/// Spec §1.3. Record length 40 B.
///
/// ```text
/// Fixed prefix only. `name[name_len]` follows at +40 and a 6-byte trailer follows the name.
/// ```
pub(crate) mod tail_directory_entry {
    /// Record length in bytes. Spec §1.3.
    pub(crate) const LEN: usize = 40;
    /// Offset of `marker` (`bytes[6]`). Spec §1.3.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `type_id` (`u32`, little-endian). Spec §1.3.
    pub(crate) const TYPE_ID: usize = 6;
    /// Offset of `zero_at_10` (`u32`, little-endian). Spec §1.3.
    pub(crate) const ZERO_AT_10: usize = 10;
    /// Offset of `size` (`u32`, little-endian). Spec §1.3.
    pub(crate) const SIZE: usize = 14;
    /// Offset of `zero_at_18` (`u32`, little-endian). Spec §1.3.
    pub(crate) const ZERO_AT_18: usize = 18;
    /// Offset of `name_len` (`u32`, little-endian). Spec §1.3.
    pub(crate) const NAME_LEN: usize = 22;
    /// Offset of `descriptor` (`bytes[14]`). Spec §1.3.
    pub(crate) const DESCRIPTOR: usize = 26;
}

/// Byte offsets for the `zlb_wrapper_header` record.
///
/// Spec §1. Record length 24 B.
///
/// ```text
/// Fixed prefix only; the zlib member follows and an 8-byte trailer closes the wrapper.
/// ```
pub(crate) mod zlb_wrapper_header {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 24;
    /// Offset of `magic` (`bytes[16]`). Spec §1.
    pub(crate) const MAGIC: usize = 0;
    /// Stated value of `magic` (`bytes[16]`). Spec §1.
    pub(crate) const MAGIC_VALUE: [u8; 16] = [
        0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef,
        0x99,
    ];
    /// Offset of `uncompressed_size` (`u32`, little-endian). Spec §1.
    pub(crate) const UNCOMPRESSED_SIZE: usize = 16;
    /// Offset of `zlib_member_size` (`u32`, little-endian). Spec §1.
    pub(crate) const ZLIB_MEMBER_SIZE: usize = 20;
}

/// Byte offsets for the `parasolid_chain_section_header` record.
///
/// Spec §1. Record length 20 B.
///
/// ```text
/// The chain length is followed by the wrapper magic; frame headers and zero padding follow.
/// ```
pub(crate) mod parasolid_chain_section_header {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 20;
    /// Offset of `chain_len` (`u32`, little-endian). Spec §1.
    pub(crate) const CHAIN_LEN: usize = 0;
    /// Offset of `magic` (`bytes[16]`). Spec §1.
    pub(crate) const MAGIC: usize = 4;
    /// Stated value of `magic` (`bytes[16]`). Spec §1.
    pub(crate) const MAGIC_VALUE: [u8; 16] = [
        0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef,
        0x99,
    ];
}

/// Byte offsets for the `parasolid_chain_frame_header` record.
///
/// Spec §1. Record length 8 B.
///
/// ```text
/// The zlib member immediately follows this fixed header and occupies the declared member size.
/// ```
pub(crate) mod parasolid_chain_frame_header {
    /// Record length in bytes. Spec §1.
    pub(crate) const LEN: usize = 8;
    /// Offset of `uncompressed_size` (`u32`, little-endian). Spec §1.
    pub(crate) const UNCOMPRESSED_SIZE: usize = 0;
    /// Offset of `zlib_member_size` (`u32`, little-endian). Spec §1.
    pub(crate) const ZLIB_MEMBER_SIZE: usize = 4;
}

/// Byte offsets for the `world_point` record.
///
/// Spec §4. Record length 38 B.
///
/// ```text
/// Offsets are body-relative, i.e. after the two-byte `00 1d` tag. Attrs `0` and `1` are sentinels, not world points.
/// ```
pub(crate) mod world_point {
    /// Record length in bytes. Spec §4.
    pub(crate) const LEN: usize = 38;
    /// Offset of `refs` (`u16[4]`, big-endian). Spec §4.
    pub(crate) const REFS: usize = 6;
    /// Offset of `xyz` (`f64[3]`, big-endian). Spec §4.
    pub(crate) const XYZ: usize = 14;
}

/// Byte offsets for the `entity_common_header` record.
///
/// Spec §5. Record length 12 B.
///
/// ```text
/// Body-relative, after the two-byte family tag. An optional `ff` byte can occur between the `00 51` tag and `flags`; it shifts every following field by one byte. `disc` points to the ATTRIB_DEF node. Bare ATTRIBUTE slots follow at +12 and total `5 + flo`.
/// ```
pub(crate) mod entity_common_header {
    /// Record length in bytes. Spec §5.
    pub(crate) const LEN: usize = 12;
    /// Offset of `flags` (`u32`, big-endian). Spec §5.
    pub(crate) const FLAGS: usize = 0;
    /// Offset of `attr` (`u16`, big-endian). Spec §5.
    pub(crate) const ATTR: usize = 4;
    /// Offset of `seq` (`u32`, big-endian). Spec §5.
    pub(crate) const SEQ: usize = 6;
    /// Offset of `disc` (`u16`, big-endian). Spec §5.
    pub(crate) const DISC: usize = 10;
}

/// Byte offsets for the `attribute_instance_00_51` record.
///
/// Spec §5. Record length 14 B.
///
/// ```text
/// Body-relative prefix. `definition_node_id` selects the same-stream ATTRIB_DEF; ATTRIBUTE slots begin at body +14. Bare framing uses `5 + flo` u16 slots; prefixed framing uses terminated `[01][hi][lo]` triples. The +0..+6 region is the common-header `flags` and `attr` of the same record.
/// ```
pub(crate) mod attribute_instance_00_51 {
    /// Record length in bytes. Spec §5.
    pub(crate) const LEN: usize = 14;
    /// Offset of `zero_selector` (`u16`, big-endian). Spec §5.
    pub(crate) const ZERO_SELECTOR: usize = 6;
    /// Offset of `definition_node_id` (`u16`, big-endian). Spec §5.
    pub(crate) const DEFINITION_NODE_ID: usize = 10;
    /// Offset of `owner_attribute_id` (`u16`, big-endian). Spec §5.
    pub(crate) const OWNER_ATTRIBUTE_ID: usize = 12;
}

/// Byte offsets for the `compact_analytic_header` record.
///
/// Spec §7.1. Record length 17 B.
///
/// ```text
/// Body-relative, after the two-byte `00 TT` tag and the optional `ff`. The partition form uses five u16 references and places the marker at +16; the deltas form uses five [hi][lo][01] reference triples and places it at +21. Values follow the marker in either form; `n` is the per-tag f64 count. All scalar payload values are finite, and no coordinate-magnitude cutoff is part of the format. A carrier is accepted only for a unique framing.
/// ```
pub(crate) mod compact_analytic_header {
    /// Record length in bytes. Spec §7.1.
    pub(crate) const LEN: usize = 17;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.1.
    pub(crate) const ATTR: usize = 0;
    /// Offset of `ordinal` (`u32`, big-endian). Spec §7.1.
    pub(crate) const ORDINAL: usize = 2;
    /// Offset of `refs` (`u16[5]`, big-endian). Spec §7.1.
    pub(crate) const REFS: usize = 6;
    /// Offset of `marker` (`u8`). Spec §7.1.
    pub(crate) const MARKER: usize = 16;
}

/// Byte offsets for the `bspline_surface_descriptor` record.
///
/// Spec §7.2. Record length 42 B.
///
/// ```text
/// Body-relative after the two-byte tag and optional marker. Offsets are relative to the descriptor attribute at +0; the terminal array references occupy +32..+41. A referenced knot array may have trailing physical entries beyond its distinct-knot count when every matching trailing multiplicity is zero; those f64 slots are ignored.
/// ```
pub(crate) mod bspline_surface_descriptor {
    /// Record length in bytes. Spec §7.2.
    pub(crate) const LEN: usize = 42;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.2.
    pub(crate) const ATTR: usize = 0;
    /// Offset of `u_periodic` (`u8`). Spec §7.2.
    pub(crate) const U_PERIODIC: usize = 2;
    /// Offset of `v_periodic` (`u8`). Spec §7.2.
    pub(crate) const V_PERIODIC: usize = 3;
    /// Offset of `u_degree` (`u16`, big-endian). Spec §7.2.
    pub(crate) const U_DEGREE: usize = 4;
    /// Offset of `v_degree` (`u16`, big-endian). Spec §7.2.
    pub(crate) const V_DEGREE: usize = 6;
    /// Offset of `u_pole_count` (`u32`, big-endian). Spec §7.2.
    pub(crate) const U_POLE_COUNT: usize = 8;
    /// Offset of `v_pole_count` (`u32`, big-endian). Spec §7.2.
    pub(crate) const V_POLE_COUNT: usize = 12;
    /// Offset of `u_knot_type` (`u8`). Spec §7.2.
    pub(crate) const U_KNOT_TYPE: usize = 16;
    /// Offset of `v_knot_type` (`u8`). Spec §7.2.
    pub(crate) const V_KNOT_TYPE: usize = 17;
    /// Offset of `u_distinct_knot_count` (`u32`, big-endian). Spec §7.2.
    pub(crate) const U_DISTINCT_KNOT_COUNT: usize = 18;
    /// Offset of `v_distinct_knot_count` (`u32`, big-endian). Spec §7.2.
    pub(crate) const V_DISTINCT_KNOT_COUNT: usize = 22;
    /// Offset of `rational` (`u8`). Spec §7.2.
    pub(crate) const RATIONAL: usize = 26;
    /// Offset of `u_closed` (`u8`). Spec §7.2.
    pub(crate) const U_CLOSED: usize = 27;
    /// Offset of `v_closed` (`u8`). Spec §7.2.
    pub(crate) const V_CLOSED: usize = 28;
    /// Offset of `surface_form` (`u8`). Spec §7.2.
    pub(crate) const SURFACE_FORM: usize = 29;
    /// Offset of `vertex_dim` (`u16`, big-endian). Spec §7.2.
    pub(crate) const VERTEX_DIM: usize = 30;
    /// Offset of `array_refs` (`u16[5]`, big-endian). Spec §7.2.
    pub(crate) const ARRAY_REFS: usize = 32;
}

/// Byte offsets for the `bspline_array_header` record.
///
/// Spec §7.2. Record length 6 B.
///
/// ```text
/// Shared header of `00 2d` (poles, f64 elements), `00 7f` (knot multiplicities, u16 elements), and `00 80` (unique knot values, f64 elements). Offsets are relative to the byte after the tag and the marker. Element data follows at +6. A surface knot array may include trailing physical entries beyond the descriptor count when every matching trailing multiplicity is zero; the extra f64 slots are ignored.
/// ```
pub(crate) mod bspline_array_header {
    /// Record length in bytes. Spec §7.2.
    pub(crate) const LEN: usize = 6;
    /// Offset of `count` (`u32`, big-endian). Spec §7.2.
    pub(crate) const COUNT: usize = 0;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.2.
    pub(crate) const ATTR: usize = 4;
}

/// Byte offsets for the `bspline_compact_array_header` record.
///
/// Spec §7.2. Record length 4 B.
///
/// ```text
/// Complete compact-array header including its leading zero byte. Element data follows at +4. The referencing descriptor role selects f64 control/knot values or u16 multiplicities.
/// ```
pub(crate) mod bspline_compact_array_header {
    /// Record length in bytes. Spec §7.2.
    pub(crate) const LEN: usize = 4;
    /// Offset of `zero` (`u8`). Spec §7.2.
    pub(crate) const ZERO: usize = 0;
    /// Offset of `count` (`u8`). Spec §7.2.
    pub(crate) const COUNT: usize = 1;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.2.
    pub(crate) const ATTR: usize = 2;
}

/// Byte offsets for the `intersection_composite` record.
///
/// Spec §7.3. Record length 29 B.
///
/// ```text
/// Body-relative, after the two-byte `00 26` tag. The intersection-data form replaces the tag with `00 01 5a` and keeps this layout unchanged.
/// ```
pub(crate) mod intersection_composite {
    /// Record length in bytes. Spec §7.3.
    pub(crate) const LEN: usize = 29;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.3.
    pub(crate) const ATTR: usize = 0;
    /// Offset of `ordinal` (`u32`, big-endian). Spec §7.3.
    pub(crate) const ORDINAL: usize = 2;
    /// Offset of `refs` (`u16[5]`, big-endian). Spec §7.3.
    pub(crate) const REFS: usize = 6;
    /// Offset of `marker` (`u8`). Spec §7.3.
    pub(crate) const MARKER: usize = 16;
    /// Offset of `payload` (`u16[6]`, big-endian). Spec §7.3.
    pub(crate) const PAYLOAD: usize = 17;
}

/// Byte offsets for the `support_uv_00_cc` record.
///
/// Spec §7.3. Record length 7 B.
///
/// ```text
/// Body-relative fixed prefix; f64 BE values follow at +7, `width` per chart point.
/// ```
pub(crate) mod support_uv_00_cc {
    /// Record length in bytes. Spec §7.3.
    pub(crate) const LEN: usize = 7;
    /// Offset of `count` (`u32`, big-endian). Spec §7.3.
    pub(crate) const COUNT: usize = 0;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.3.
    pub(crate) const ATTR: usize = 4;
    /// Offset of `width` (`u8`). Spec §7.3.
    pub(crate) const WIDTH: usize = 6;
}

/// Byte offsets for the `rolling_ball_blend_00_38` record.
///
/// Spec §7.4. Record length 56 B.
///
/// ```text
/// Body-relative, after the two-byte tag and the optional `ff`. `abs(offset0) == abs(offset1) > 0`; their common magnitude is the constant rolling-ball radius. Each `side` value is exactly `+1` or `-1`.
/// ```
pub(crate) mod rolling_ball_blend_00_38 {
    /// Record length in bytes. Spec §7.4.
    pub(crate) const LEN: usize = 56;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.4.
    pub(crate) const ATTR: usize = 0;
    /// Offset of `ordinal` (`u32`, big-endian). Spec §7.4.
    pub(crate) const ORDINAL: usize = 2;
    /// Offset of `refs` (`u16[5]`, big-endian). Spec §7.4.
    pub(crate) const REFS: usize = 6;
    /// Offset of `marker` (`u8`). Spec §7.4.
    pub(crate) const MARKER: usize = 16;
    /// Offset of `selector` (`u8`). Spec §7.4.
    pub(crate) const SELECTOR: usize = 17;
    /// Offset of `support0` (`u16`, big-endian). Spec §7.4.
    pub(crate) const SUPPORT0: usize = 18;
    /// Offset of `support1` (`u16`, big-endian). Spec §7.4.
    pub(crate) const SUPPORT1: usize = 20;
    /// Offset of `spine` (`u16`, big-endian). Spec §7.4.
    pub(crate) const SPINE: usize = 22;
    /// Offset of `offset0` (`f64`, big-endian). Spec §7.4.
    pub(crate) const OFFSET0: usize = 24;
    /// Offset of `offset1` (`f64`, big-endian). Spec §7.4.
    pub(crate) const OFFSET1: usize = 32;
    /// Offset of `side0` (`f64`, big-endian). Spec §7.4.
    pub(crate) const SIDE0: usize = 40;
    /// Offset of `side1` (`f64`, big-endian). Spec §7.4.
    pub(crate) const SIDE1: usize = 48;
}

/// Byte offsets for the `offset_surface_00_3c` record.
///
/// Spec §7.5. Record length 29 B.
///
/// ```text
/// Partition body relative to the byte after the `00 3c` tag and optional `ff`. With the two-byte tag, the compact record is 31 bytes. The deltas form expands each reference, including `support`, to `[hi][lo][01]`.
/// ```
pub(crate) mod offset_surface_00_3c {
    /// Record length in bytes. Spec §7.5.
    pub(crate) const LEN: usize = 29;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.5.
    pub(crate) const ATTR: usize = 0;
    /// Offset of `ordinal` (`u32`, big-endian). Spec §7.5.
    pub(crate) const ORDINAL: usize = 2;
    /// Offset of `refs` (`u16[5]`, big-endian). Spec §7.5.
    pub(crate) const REFS: usize = 6;
    /// Offset of `marker` (`u8`). Spec §7.5.
    pub(crate) const MARKER: usize = 16;
    /// Offset of `discriminator` (`u8`). Spec §7.5.
    pub(crate) const DISCRIMINATOR: usize = 17;
    /// Offset of `true_offset` (`u8`). Spec §7.5.
    pub(crate) const TRUE_OFFSET: usize = 18;
    /// Offset of `support` (`u16`, big-endian). Spec §7.5.
    pub(crate) const SUPPORT: usize = 19;
    /// Offset of `distance` (`f64`, big-endian). Spec §7.5.
    pub(crate) const DISTANCE: usize = 21;
}

/// Byte offsets for the `current_extended_zero_tail_92_profile_curve` record.
///
/// Spec §2. Record length 92 B.
///
/// ```text
/// Endpoint values are zero-based positions in the feature-owned coordinate-bearing geometry roster; the following bytes may be relation payload.
/// ```
pub(crate) mod current_extended_zero_tail_92_profile_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 92;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[12]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Stated value of `header` (`bytes[12]`). Spec §2.
    pub(crate) const HEADER_VALUE: [u8; 12] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x80, 0xbf,
    ];
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Stated value of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS_VALUE: [u8; 4] = [0x04, 0x00, 0x02, 0x00];
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0001;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Stated value of `state` (`u16`). Spec §2.
    pub(crate) const STATE_VALUE: u16 = 0x0001;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Stated value of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR_VALUE: [u8; 8] = [0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00];
    /// Offset of `state_scalar` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_SCALAR: usize = 48;
    /// Stated value of `state_scalar` (`f64`). Spec §2.
    pub(crate) const STATE_SCALAR_VALUE: f64 = 1.0;
    /// Offset of `zero_endpoint_prefix` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX: usize = 56;
    /// Stated value of `zero_endpoint_prefix` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX_VALUE: [u8; 8] =
        [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 64;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 66;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 68;
    /// Stated value of `endpoint_selector` (`u32`). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR_VALUE: u32 = 0x0000_0001;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 72;
    /// Stated value of `signed_selector` (`f64`). Spec §2.
    pub(crate) const SIGNED_SELECTOR_VALUE: f64 = -1.0;
    /// Offset of `zero_tail` (`bytes[12]`). Spec §2.
    pub(crate) const ZERO_TAIL: usize = 80;
    /// Stated value of `zero_tail` (`bytes[12]`). Spec §2.
    pub(crate) const ZERO_TAIL_VALUE: [u8; 12] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
}

/// Byte offsets for the `extended_wide_104_profile_curve` record.
///
/// Spec §2. Record length 104 B.
///
/// ```text
/// The endpoint fields are zero-based ordinals in the feature-owned coordinate-bearing geometry roster.
/// ```
pub(crate) mod extended_wide_104_profile_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 104;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `profile_selector` (`bytes[8]`). Spec §2.
    pub(crate) const PROFILE_SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `zero_endpoint_prefix` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX: usize = 56;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 64;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 66;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 68;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 72;
    /// Offset of `zero_trailer_prefix` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_TRAILER_PREFIX: usize = 80;
    /// Offset of `trailer_tag0` (`bytes[4]`). Spec §2.
    pub(crate) const TRAILER_TAG0: usize = 88;
    /// Offset of `trailer_tag1` (`bytes[4]`). Spec §2.
    pub(crate) const TRAILER_TAG1: usize = 92;
    /// Offset of `zero_trailer_suffix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_TRAILER_SUFFIX: usize = 96;
    /// Offset of `identity` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY: usize = 100;
}

/// Byte offsets for the `legacy_wide_112_profile_roster_curve` record.
///
/// Spec §2. Record length 112 B.
///
/// ```text
/// The endpoint fields are zero-based ordinals in the complete feature-owned coordinate-bearing geometry roster; this state-zero trailer is distinct from the object-index wide curve trailer.
/// ```
pub(crate) mod legacy_wide_112_profile_roster_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 112;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `zero_endpoint_prefix` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX: usize = 56;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 64;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 66;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 68;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 72;
    /// Offset of `trailer_selector` (`i32`, little-endian). Spec §2.
    pub(crate) const TRAILER_SELECTOR: usize = 80;
    /// Offset of `local_state` (`u16`, little-endian). Spec §2.
    pub(crate) const LOCAL_STATE: usize = 84;
    /// Offset of `reference_sentinels` (`i32[4]`, little-endian). Spec §2.
    pub(crate) const REFERENCE_SENTINELS: usize = 86;
    /// Offset of `zero_trailer` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 102;
    /// Offset of `identity_first` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_FIRST: usize = 104;
    /// Offset of `identity_second` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_SECOND: usize = 108;
}

/// Byte offsets for the `legacy_wide_104_profile_roster_curve` record.
///
/// Spec §2. Record length 104 B.
///
/// ```text
/// The endpoint fields are zero-based ordinals in the complete feature-owned coordinate-bearing geometry roster.
/// ```
pub(crate) mod legacy_wide_104_profile_roster_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 104;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `zero_endpoint_prefix` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX: usize = 56;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 64;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 66;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 68;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 72;
    /// Offset of `zero_trailer_prefix` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_TRAILER_PREFIX: usize = 80;
    /// Offset of `local_id` (`u32`, little-endian). Spec §2.
    pub(crate) const LOCAL_ID: usize = 88;
    /// Offset of `zero_trailer_gap` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_TRAILER_GAP: usize = 92;
    /// Offset of `trailer_tag` (`u32`, little-endian). Spec §2.
    pub(crate) const TRAILER_TAG: usize = 96;
    /// Offset of `next_object_index` (`u32`, little-endian). Spec §2.
    pub(crate) const NEXT_OBJECT_INDEX: usize = 100;
}

/// Byte offsets for the `extended_geometry_104_indexed_arc` record.
///
/// Spec §2. Record length 104 B.
///
/// ```text
/// Distinct endpoint indices define a minor arc; equal endpoint indices use the extended full-circle layout.
/// ```
pub(crate) mod extended_geometry_104_indexed_arc {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 104;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 60;
    /// Offset of `signed_radius_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_RADIUS_SELECTOR: usize = 64;
    /// Offset of `arc_selector` (`i32`, little-endian). Spec §2.
    pub(crate) const ARC_SELECTOR: usize = 72;
    /// Offset of `center_index` (`u16`, little-endian). Spec §2.
    pub(crate) const CENTER_INDEX: usize = 76;
    /// Offset of `reference_sentinels` (`bytes[16]`). Spec §2.
    pub(crate) const REFERENCE_SENTINELS: usize = 78;
    /// Offset of `terminator` (`u16`, little-endian). Spec §2.
    pub(crate) const TERMINATOR: usize = 94;
    /// Offset of `trailer_identities` (`u32[2]`, little-endian). Spec §2.
    pub(crate) const TRAILER_IDENTITIES: usize = 96;
}

/// Byte offsets for the `extended_profile_104_indexed_arc` record.
///
/// Spec §2. Record length 104 B.
///
/// ```text
/// Distinct endpoint indices define a minor arc; one less than the smaller endpoint index selects the center under the profile-locus fallback rules.
/// ```
pub(crate) mod extended_profile_104_indexed_arc {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 104;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Stated value of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS_VALUE: [u8; 4] = [0x04, 0x00, 0x02, 0x00];
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0001;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Stated value of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR_VALUE: [u8; 8] = [0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00];
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 60;
    /// Stated value of `endpoint_selector` (`u32`). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR_VALUE: u32 = 0x0000_0001;
    /// Offset of `signed_radius_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_RADIUS_SELECTOR: usize = 64;
    /// Stated value of `signed_radius_selector` (`f64`). Spec §2.
    pub(crate) const SIGNED_RADIUS_SELECTOR_VALUE: f64 = -1.0;
    /// Offset of `arc_selector` (`i32`, little-endian). Spec §2.
    pub(crate) const ARC_SELECTOR: usize = 72;
    /// Offset of `reference_sentinels` (`bytes[16]`). Spec §2.
    pub(crate) const REFERENCE_SENTINELS: usize = 78;
    /// Stated value of `reference_sentinels` (`bytes[16]`). Spec §2.
    pub(crate) const REFERENCE_SENTINELS_VALUE: [u8; 16] = [
        0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff,
        0xfe,
    ];
    /// Offset of `terminator` (`u16`, little-endian). Spec §2.
    pub(crate) const TERMINATOR: usize = 94;
    /// Stated value of `terminator` (`u16`). Spec §2.
    pub(crate) const TERMINATOR_VALUE: u16 = 0x0000;
    /// Offset of `trailer_identities` (`u32[2]`, little-endian). Spec §2.
    pub(crate) const TRAILER_IDENTITIES: usize = 96;
}

/// Byte offsets for the `extended_profile_terminal_102_indexed_arc` record.
///
/// Spec §2. Record length 102 B.
///
/// ```text
/// The terminal record uses the same center resolution as the 104-byte compact indexed profile arc and has no following sketch marker.
/// ```
pub(crate) mod extended_profile_terminal_102_indexed_arc {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 102;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Stated value of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS_VALUE: [u8; 4] = [0x04, 0x00, 0x02, 0x00];
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0001;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Stated value of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR_VALUE: [u8; 8] = [0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00];
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 60;
    /// Stated value of `endpoint_selector` (`u32`). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR_VALUE: u32 = 0x0000_0001;
    /// Offset of `signed_radius_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_RADIUS_SELECTOR: usize = 64;
    /// Stated value of `signed_radius_selector` (`f64`). Spec §2.
    pub(crate) const SIGNED_RADIUS_SELECTOR_VALUE: f64 = -1.0;
    /// Offset of `arc_selector` (`i32`, little-endian). Spec §2.
    pub(crate) const ARC_SELECTOR: usize = 72;
    /// Offset of `reference_sentinels` (`bytes[16]`). Spec §2.
    pub(crate) const REFERENCE_SENTINELS: usize = 78;
    /// Stated value of `reference_sentinels` (`bytes[16]`). Spec §2.
    pub(crate) const REFERENCE_SENTINELS_VALUE: [u8; 16] = [
        0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff,
        0xfe,
    ];
    /// Offset of `zero_tail` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_TAIL: usize = 94;
    /// Stated value of `zero_tail` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_TAIL_VALUE: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
}

/// Byte offsets for the `extended_geometry_116_indexed_arc` record.
///
/// Spec §2. Record length 116 B.
///
/// ```text
/// The relation tail is bounded by the following sketch marker at +116.
/// ```
pub(crate) mod extended_geometry_116_indexed_arc {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 116;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 60;
    /// Offset of `signed_radius_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_RADIUS_SELECTOR: usize = 64;
    /// Offset of `arc_selector` (`i32`, little-endian). Spec §2.
    pub(crate) const ARC_SELECTOR: usize = 72;
    /// Offset of `center_index` (`u16`, little-endian). Spec §2.
    pub(crate) const CENTER_INDEX: usize = 76;
    /// Offset of `reference_sentinels` (`bytes[16]`). Spec §2.
    pub(crate) const REFERENCE_SENTINELS: usize = 78;
    /// Offset of `relation_tail_padding` (`bytes[8]`). Spec §2.
    pub(crate) const RELATION_TAIL_PADDING: usize = 94;
    /// Offset of `relation_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const RELATION_KIND: usize = 102;
    /// Offset of `relation_padding` (`bytes[6]`). Spec §2.
    pub(crate) const RELATION_PADDING: usize = 106;
    /// Offset of `following_object_index` (`u32`, little-endian). Spec §2.
    pub(crate) const FOLLOWING_OBJECT_INDEX: usize = 112;
}

/// Byte offsets for the `compact_indexed_curve_continuation120` record.
///
/// Spec §2. Record length 122 B.
///
/// ```text
/// A valid class declaration may begin at the record boundary +122.
/// ```
pub(crate) mod compact_indexed_curve_continuation120 {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 122;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `locus` (`bytes[4]`). Spec §2.
    pub(crate) const LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 60;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 64;
    /// Offset of `continuation_padding` (`bytes[48]`). Spec §2.
    pub(crate) const CONTINUATION_PADDING: usize = 72;
    /// Offset of `continuation_kind` (`u16`, little-endian). Spec §2.
    pub(crate) const CONTINUATION_KIND: usize = 120;
}

/// Byte offsets for the `compact_legacy_140_relation_display_curve` record.
///
/// Spec §2. Record length 140 B.
///
/// ```text
/// The endpoint ordinals use the complete feature-local marker roster; relation endpoints make this a display carrier.
/// ```
pub(crate) mod compact_legacy_140_relation_display_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 140;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Stated value of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER_VALUE: [u8; 8] = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    /// Offset of `shared_selector` (`bytes[4]`). Spec §2.
    pub(crate) const SHARED_SELECTOR: usize = 13;
    /// Stated value of `shared_selector` (`bytes[4]`). Spec §2.
    pub(crate) const SHARED_SELECTOR_VALUE: [u8; 4] = [0x00, 0x00, 0x80, 0xbf];
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Stated value of `native_kind` (`u32`). Spec §2.
    pub(crate) const NATIVE_KIND_VALUE: u32 = 0x0000_0001;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Stated value of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS_VALUE: [u8; 4] = [0x04, 0x00, 0x02, 0x00];
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0001;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Stated value of `state` (`u16`). Spec §2.
    pub(crate) const STATE_VALUE: u16 = 0x0001;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Stated value of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR_VALUE: [u8; 8] = [0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00];
    /// Offset of `state_scalar` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_SCALAR: usize = 48;
    /// Stated value of `state_scalar` (`f64`). Spec §2.
    pub(crate) const STATE_SCALAR_VALUE: f64 = 1.0;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 60;
    /// Stated value of `endpoint_selector` (`u32`). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR_VALUE: u32 = 0x0000_0001;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 64;
    /// Stated value of `signed_selector` (`f64`). Spec §2.
    pub(crate) const SIGNED_SELECTOR_VALUE: f64 = -1.0;
    /// Offset of `continuation_padding` (`bytes[48]`). Spec §2.
    pub(crate) const CONTINUATION_PADDING: usize = 72;
    /// Offset of `continuation_kind` (`u16`, little-endian). Spec §2.
    pub(crate) const CONTINUATION_KIND: usize = 120;
    /// Offset of `continuation_selector` (`bytes[2]`). Spec §2.
    pub(crate) const CONTINUATION_SELECTOR: usize = 122;
    /// Offset of `zero_selector_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_SELECTOR_PREFIX: usize = 124;
    /// Stated value of `zero_selector_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_SELECTOR_PREFIX_VALUE: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    /// Offset of `relation_selectors` (`bytes[4]`). Spec §2.
    pub(crate) const RELATION_SELECTORS: usize = 128;
    /// Offset of `continuation_tail` (`bytes[8]`). Spec §2.
    pub(crate) const CONTINUATION_TAIL: usize = 132;
    /// Stated value of `continuation_tail` (`bytes[8]`). Spec §2.
    pub(crate) const CONTINUATION_TAIL_VALUE: [u8; 8] =
        [0xff, 0xfe, 0xff, 0x02, 0x44, 0x00, 0x31, 0x00];
}

/// Byte offsets for the `current_terminal_relation_carrier` record.
///
/// Spec §2. Record length 136 B.
///
/// ```text
/// The class declaration begins at the record boundary +136 and is owned by the matching feature relation.
/// ```
pub(crate) mod current_terminal_relation_carrier {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 136;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `zero_endpoint_prefix` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX: usize = 56;
    /// Offset of `terminal_header` (`bytes[4]`). Spec §2.
    pub(crate) const TERMINAL_HEADER: usize = 64;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 68;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 72;
    /// Offset of `terminal_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const TERMINAL_SELECTOR: usize = 80;
    /// Offset of `terminal_state` (`u16`, little-endian). Spec §2.
    pub(crate) const TERMINAL_STATE: usize = 84;
    /// Offset of `reference_sentinels` (`bytes[16]`). Spec §2.
    pub(crate) const REFERENCE_SENTINELS: usize = 86;
    /// Offset of `zero_tail` (`bytes[32]`). Spec §2.
    pub(crate) const ZERO_TAIL: usize = 102;
    /// Offset of `terminal_tag` (`u16`, little-endian). Spec §2.
    pub(crate) const TERMINAL_TAG: usize = 134;
}

/// Byte offsets for the `extended_geometry_terminal_circle_dimension_tail` record.
///
/// Spec §2. Record length 160 B.
///
/// ```text
/// The equal-index circle uses the preceding entry in the complete coordinate-bearing geometry roster as its center.
/// ```
pub(crate) mod extended_geometry_terminal_circle_dimension_tail {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 160;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 60;
    /// Offset of `signed_radius_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_RADIUS_SELECTOR: usize = 64;
    /// Offset of `arc_selector` (`i32`, little-endian). Spec §2.
    pub(crate) const ARC_SELECTOR: usize = 72;
    /// Offset of `auxiliary_index` (`u16`, little-endian). Spec §2.
    pub(crate) const AUXILIARY_INDEX: usize = 76;
    /// Offset of `reference_sentinels` (`bytes[16]`). Spec §2.
    pub(crate) const REFERENCE_SENTINELS: usize = 78;
    /// Offset of `terminal_padding` (`bytes[34]`). Spec §2.
    pub(crate) const TERMINAL_PADDING: usize = 94;
    /// Offset of `dimension_kind` (`u16`, little-endian). Spec §2.
    pub(crate) const DIMENSION_KIND: usize = 128;
    /// Offset of `reference` (`u32`, little-endian). Spec §2.
    pub(crate) const REFERENCE: usize = 130;
    /// Offset of `dimension_state` (`u16`, little-endian). Spec §2.
    pub(crate) const DIMENSION_STATE: usize = 134;
    /// Offset of `dimension_value` (`u32`, little-endian). Spec §2.
    pub(crate) const DIMENSION_VALUE: usize = 136;
    /// Offset of `dimension_suffix` (`bytes[8]`). Spec §2.
    pub(crate) const DIMENSION_SUFFIX: usize = 140;
    /// Offset of `trailing_value` (`f64`, little-endian). Spec §2.
    pub(crate) const TRAILING_VALUE: usize = 148;
    /// Offset of `terminal_sentinel` (`bytes[4]`). Spec §2.
    pub(crate) const TERMINAL_SENTINEL: usize = 156;
}

/// Byte offsets for the `extended_selector44_indexed_line_continuation` record.
///
/// Spec §2. Record length 84 B.
///
/// ```text
/// The endpoint fields are zero-based indices in the feature-owned coordinate-bearing point roster.
/// ```
pub(crate) mod extended_selector44_indexed_line_continuation {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 84;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `locus` (`bytes[4]`). Spec §2.
    pub(crate) const LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `continuation_header` (`bytes[9]`). Spec §2.
    pub(crate) const CONTINUATION_HEADER: usize = 39;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 60;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 64;
    /// Offset of `continuation_body` (`bytes[12]`). Spec §2.
    pub(crate) const CONTINUATION_BODY: usize = 72;
}

/// Byte offsets for the `extended_selector44_indexed_line_control_terminal` record.
///
/// Spec §2. Record length 170 B.
///
/// ```text
/// The endpoint fields are zero-based indices in the feature-owned coordinate-bearing point roster.
/// ```
pub(crate) mod extended_selector44_indexed_line_control_terminal {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 170;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `locus` (`bytes[4]`). Spec §2.
    pub(crate) const LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `terminal_prefix` (`bytes[9]`). Spec §2.
    pub(crate) const TERMINAL_PREFIX: usize = 39;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 60;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 64;
    /// Offset of `terminal_padding` (`bytes[70]`). Spec §2.
    pub(crate) const TERMINAL_PADDING: usize = 72;
    /// Offset of `terminal_tag` (`bytes[2]`). Spec §2.
    pub(crate) const TERMINAL_TAG: usize = 142;
    /// Offset of `terminal_suffix` (`bytes[10]`). Spec §2.
    pub(crate) const TERMINAL_SUFFIX: usize = 144;
    /// Offset of `control_sequence` (`bytes[16]`). Spec §2.
    pub(crate) const CONTROL_SEQUENCE: usize = 154;
}

/// Byte offsets for the `extended_terminal_164_wide_profile_curve` record.
///
/// Spec §2. Record length 164 B.
///
/// ```text
/// The endpoint fields are zero-based ordinals in the feature-owned coordinate-bearing geometry roster.
/// ```
pub(crate) mod extended_terminal_164_wide_profile_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 164;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `zero_endpoint_prefix` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX: usize = 56;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 64;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 66;
    /// Offset of `endpoint_selector` (`u32`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SELECTOR: usize = 68;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 72;
    /// Offset of `zero_trailer` (`bytes[54]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 80;
    /// Offset of `terminal_state` (`u16`, little-endian). Spec §2.
    pub(crate) const TERMINAL_STATE: usize = 134;
    /// Offset of `zero_terminal_padding` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_TERMINAL_PADDING: usize = 136;
    /// Offset of `null_identity` (`u32`, little-endian). Spec §2.
    pub(crate) const NULL_IDENTITY: usize = 144;
    /// Offset of `zero_terminal_suffix` (`bytes[16]`). Spec §2.
    pub(crate) const ZERO_TERMINAL_SUFFIX: usize = 148;
}

/// Byte offsets for the `legacy_140_single_incidence_profile_point` record.
///
/// Spec §2. Record length 140 B.
///
/// ```text
/// The record emits a point. The fixed layout includes the single-incidence and shared-f32 variants with their complete identity trailers.
/// ```
pub(crate) mod legacy_140_single_incidence_profile_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 140;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 21;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `zero_state` (`u16`, little-endian). Spec §2.
    pub(crate) const ZERO_STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `zero_state_prefix` (`bytes[9]`). Spec §2.
    pub(crate) const ZERO_STATE_PREFIX: usize = 39;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 58;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 66;
    /// Offset of `zero_link_prefix` (`u16`, little-endian). Spec §2.
    pub(crate) const ZERO_LINK_PREFIX: usize = 74;
    /// Offset of `link_state` (`u16`, little-endian). Spec §2.
    pub(crate) const LINK_STATE: usize = 76;
    /// Offset of `incidence_cell` (`bytes[12]`). Spec §2.
    pub(crate) const INCIDENCE_CELL: usize = 78;
    /// Offset of `link_terminator` (`bytes[6]`). Spec §2.
    pub(crate) const LINK_TERMINATOR: usize = 90;
    /// Offset of `trailer_prefix` (`bytes[32]`). Spec §2.
    pub(crate) const TRAILER_PREFIX: usize = 96;
    /// Offset of `identity_first` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_FIRST: usize = 128;
    /// Offset of `trailer_middle` (`bytes[4]`). Spec §2.
    pub(crate) const TRAILER_MIDDLE: usize = 132;
    /// Offset of `identity_second` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_SECOND: usize = 136;
}

/// Byte offsets for the `legacy_144_single_incidence_profile_point` record.
///
/// Spec §2. Record length 144 B.
///
/// ```text
/// The record emits a point. Its terminal identity and next-marker boundary are four bytes beyond the 140-byte shared-f32 form.
/// ```
pub(crate) mod legacy_144_single_incidence_profile_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 144;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Stated value of `native_kind` (`u32`). Spec §2.
    pub(crate) const NATIVE_KIND_VALUE: u32 = 0x0000_0001;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 21;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0001;
    /// Offset of `zero_state` (`u16`, little-endian). Spec §2.
    pub(crate) const ZERO_STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `zero_state_prefix` (`bytes[9]`). Spec §2.
    pub(crate) const ZERO_STATE_PREFIX: usize = 39;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 58;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 66;
    /// Offset of `zero_link_prefix` (`u16`, little-endian). Spec §2.
    pub(crate) const ZERO_LINK_PREFIX: usize = 74;
    /// Offset of `link_state` (`u16`, little-endian). Spec §2.
    pub(crate) const LINK_STATE: usize = 76;
    /// Offset of `incidence_cell` (`bytes[12]`). Spec §2.
    pub(crate) const INCIDENCE_CELL: usize = 78;
    /// Offset of `zero_post_cell` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_POST_CELL: usize = 90;
    /// Offset of `link_terminator` (`bytes[6]`). Spec §2.
    pub(crate) const LINK_TERMINATOR: usize = 94;
    /// Offset of `trailer_prefix` (`bytes[40]`). Spec §2.
    pub(crate) const TRAILER_PREFIX: usize = 100;
    /// Offset of `identity` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY: usize = 140;
}

/// Byte offsets for the `legacy_geometry_locus_alternate_134_point` record.
///
/// Spec §2. Record length 134 B.
///
/// ```text
/// The two fixed tails select an alternate-tag coordinate point; the next sketch marker begins at +134.
/// ```
pub(crate) mod legacy_geometry_locus_alternate_134_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 134;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 21;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `zero_before_state_value` (`bytes[9]`). Spec §2.
    pub(crate) const ZERO_BEFORE_STATE_VALUE: usize = 39;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 58;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 66;
    /// Offset of `tail_selector` (`bytes[10]`). Spec §2.
    pub(crate) const TAIL_SELECTOR: usize = 74;
    /// Offset of `tail_sentinel` (`i32`, little-endian). Spec §2.
    pub(crate) const TAIL_SENTINEL: usize = 84;
    /// Stated value of `tail_sentinel` (`i32`). Spec §2.
    pub(crate) const TAIL_SENTINEL_VALUE: i32 = -2;
    /// Offset of `zero_trailer` (`bytes[42]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 88;
    /// Offset of `identity` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY: usize = 130;
}

/// Byte offsets for the `legacy_geometry_locus_alternate_138_point` record.
///
/// Spec §2. Record length 138 B.
///
/// ```text
/// The repeated identity at +134 is the following marker's object identifier.
/// ```
pub(crate) mod legacy_geometry_locus_alternate_138_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 138;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 21;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `zero_before_state_value` (`bytes[9]`). Spec §2.
    pub(crate) const ZERO_BEFORE_STATE_VALUE: usize = 39;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 58;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 66;
    /// Offset of `zero_and_state` (`bytes[10]`). Spec §2.
    pub(crate) const ZERO_AND_STATE: usize = 74;
    /// Offset of `tail_sentinel` (`i32`, little-endian). Spec §2.
    pub(crate) const TAIL_SENTINEL: usize = 84;
    /// Stated value of `tail_sentinel` (`i32`). Spec §2.
    pub(crate) const TAIL_SENTINEL_VALUE: i32 = -2;
    /// Offset of `zero_identity_prefix` (`bytes[36]`). Spec §2.
    pub(crate) const ZERO_IDENTITY_PREFIX: usize = 88;
    /// Offset of `identity_first` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_FIRST: usize = 124;
    /// Offset of `zero_before_identity_second` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_BEFORE_IDENTITY_SECOND: usize = 128;
    /// Offset of `identity_second` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_SECOND: usize = 130;
    /// Offset of `following_identity` (`u32`, little-endian). Spec §2.
    pub(crate) const FOLLOWING_IDENTITY: usize = 134;
}

/// Byte offsets for the `legacy_geometry_locus_alternate_154_point` record.
///
/// Spec §2. Record length 154 B.
///
/// ```text
/// The two mixed-selector incidence cells identify the point record; they do not define curve endpoints.
/// ```
pub(crate) mod legacy_geometry_locus_alternate_154_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 154;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 21;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `zero_before_state_value` (`bytes[9]`). Spec §2.
    pub(crate) const ZERO_BEFORE_STATE_VALUE: usize = 39;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 58;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 66;
    /// Offset of `zero_link_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_LINK_PREFIX: usize = 74;
    /// Offset of `link_count` (`u16`, little-endian). Spec §2.
    pub(crate) const LINK_COUNT: usize = 76;
    /// Stated value of `link_count` (`u16`). Spec §2.
    pub(crate) const LINK_COUNT_VALUE: u16 = 0x0002;
    /// Offset of `incidence_first` (`bytes[12]`). Spec §2.
    pub(crate) const INCIDENCE_FIRST: usize = 78;
    /// Offset of `incidence_second` (`bytes[12]`). Spec §2.
    pub(crate) const INCIDENCE_SECOND: usize = 90;
    /// Offset of `link_terminator` (`bytes[6]`). Spec §2.
    pub(crate) const LINK_TERMINATOR: usize = 102;
    /// Offset of `zero_trailer` (`bytes[42]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 108;
    /// Offset of `record_identity` (`u32`, little-endian). Spec §2.
    pub(crate) const RECORD_IDENTITY: usize = 150;
}

/// Byte offsets for the `extended_scaled_146_profile_point` record.
///
/// Spec §2. Record length 146 B.
///
/// ```text
/// The record emits a point; its link count is twice the trailer state.
/// ```
pub(crate) mod extended_scaled_146_profile_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 146;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state_at_29` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE_AT_29: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 58;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 66;
    /// Offset of `zero_link_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_LINK_PREFIX: usize = 74;
    /// Offset of `link_count` (`u16`, little-endian). Spec §2.
    pub(crate) const LINK_COUNT: usize = 76;
    /// Offset of `incidence_first` (`bytes[8]`). Spec §2.
    pub(crate) const INCIDENCE_FIRST: usize = 78;
    /// Offset of `incidence_second` (`bytes[8]`). Spec §2.
    pub(crate) const INCIDENCE_SECOND: usize = 86;
    /// Offset of `link_terminator` (`bytes[6]`). Spec §2.
    pub(crate) const LINK_TERMINATOR: usize = 94;
    /// Offset of `zero_trailer_prefix` (`bytes[34]`). Spec §2.
    pub(crate) const ZERO_TRAILER_PREFIX: usize = 100;
    /// Offset of `trailer_state` (`u16`, little-endian). Spec §2.
    pub(crate) const TRAILER_STATE: usize = 134;
    /// Offset of `zero_trailer_suffix` (`bytes[6]`). Spec §2.
    pub(crate) const ZERO_TRAILER_SUFFIX: usize = 136;
    /// Offset of `identity` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY: usize = 142;
}

/// Byte offsets for the `extended_four_link_state_profile_point_prefix` record.
///
/// Spec §2.
///
/// ```text
/// The fixed prefix emits a point; the trailer is variable and carries a family-specific marker prefix.
/// ```
pub(crate) mod extended_four_link_state_profile_point_prefix {
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state_at_29` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE_AT_29: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 58;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 66;
    /// Offset of `zero_link_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_LINK_PREFIX: usize = 74;
    /// Offset of `link_count` (`u16`, little-endian). Spec §2.
    pub(crate) const LINK_COUNT: usize = 76;
    /// Offset of `incidence_first` (`bytes[8]`). Spec §2.
    pub(crate) const INCIDENCE_FIRST: usize = 78;
    /// Offset of `incidence_second` (`bytes[8]`). Spec §2.
    pub(crate) const INCIDENCE_SECOND: usize = 86;
    /// Offset of `link_terminator` (`bytes[6]`). Spec §2.
    pub(crate) const LINK_TERMINATOR: usize = 94;
    /// Offset of `zero_trailer_prefix` (`bytes[34]`). Spec §2.
    pub(crate) const ZERO_TRAILER_PREFIX: usize = 100;
    /// Offset of `trailer_state` (`u16`, little-endian). Spec §2.
    pub(crate) const TRAILER_STATE: usize = 134;
}

/// Byte offsets for the `extended_geometry_locus_138_point` record.
///
/// Spec §2. Record length 138 B.
///
/// ```text
/// The finite pair is a point coordinate; the two identity words are distinct record identities.
/// ```
pub(crate) mod extended_geometry_locus_138_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 138;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state_at_29` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE_AT_29: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 58;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 66;
    /// Offset of `state_at_74` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE_AT_74: usize = 74;
    /// Offset of `link_count` (`u16`, little-endian). Spec §2.
    pub(crate) const LINK_COUNT: usize = 76;
    /// Offset of `zero_link_cell` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_LINK_CELL: usize = 78;
    /// Offset of `link_sentinel` (`i32`, little-endian). Spec §2.
    pub(crate) const LINK_SENTINEL: usize = 82;
    /// Offset of `zero_trailer` (`bytes[38]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 86;
    /// Offset of `identity_first` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_FIRST: usize = 124;
    /// Offset of `identity_second` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_SECOND: usize = 128;
    /// Offset of `identity_terminator` (`bytes[6]`). Spec §2.
    pub(crate) const IDENTITY_TERMINATOR: usize = 132;
}

/// Byte offsets for the `extended_geometry_locus_96_construction_line` record.
///
/// Spec §2. Record length 96 B.
///
/// ```text
/// The endpoint fields are direct feature-local object identifiers.
/// ```
pub(crate) mod extended_geometry_locus_96_construction_line {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 96;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `zero_endpoint_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX: usize = 60;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 64;
    /// Offset of `zero_selector_trailer` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_SELECTOR_TRAILER: usize = 72;
    /// Offset of `tail_tag` (`bytes[4]`). Spec §2.
    pub(crate) const TAIL_TAG: usize = 80;
    /// Offset of `zero_tail_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_TAIL_PREFIX: usize = 84;
    /// Offset of `identity_first` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_FIRST: usize = 88;
    /// Offset of `identity_second` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_SECOND: usize = 92;
}

/// Byte offsets for the `compact_legacy_96_profile_roster_curve` record.
///
/// Spec §2. Record length 96 B.
///
/// ```text
/// The endpoint fields are zero-based ordinals in the complete feature-local coordinate-bearing geometry-marker roster.
/// ```
pub(crate) mod compact_legacy_96_profile_roster_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 96;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `shared_selector` (`f32`, little-endian). Spec §2.
    pub(crate) const SHARED_SELECTOR: usize = 13;
    /// Stated value of `shared_selector` (`f32`). Spec §2.
    pub(crate) const SHARED_SELECTOR_VALUE: f32 = -1.0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Stated value of `native_kind` (`u32`). Spec §2.
    pub(crate) const NATIVE_KIND_VALUE: u32 = 0x0000_0001;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0001;
    /// Offset of `state_at_29` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE_AT_29: usize = 29;
    /// Stated value of `state_at_29` (`u16`). Spec §2.
    pub(crate) const STATE_AT_29_VALUE: u16 = 0x0000;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `zero_endpoint_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX: usize = 60;
    /// Stated value of `zero_endpoint_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX_VALUE: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 64;
    /// Stated value of `signed_selector` (`f64`). Spec §2.
    pub(crate) const SIGNED_SELECTOR_VALUE: f64 = -1.0;
    /// Offset of `zero_selector_trailer` (`bytes[10]`). Spec §2.
    pub(crate) const ZERO_SELECTOR_TRAILER: usize = 72;
    /// Stated value of `zero_selector_trailer` (`bytes[10]`). Spec §2.
    pub(crate) const ZERO_SELECTOR_TRAILER_VALUE: [u8; 10] =
        [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    /// Offset of `tail_state` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_STATE: usize = 82;
    /// Offset of `tail_state_prefix` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_STATE_PREFIX: usize = 84;
    /// Stated value of `tail_state_prefix` (`u16`). Spec §2.
    pub(crate) const TAIL_STATE_PREFIX_VALUE: u16 = 0x0000;
    /// Offset of `tail_state_marker` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_STATE_MARKER: usize = 86;
    /// Stated value of `tail_state_marker` (`u16`). Spec §2.
    pub(crate) const TAIL_STATE_MARKER_VALUE: u16 = 0x0001;
    /// Offset of `zero_tail_identity` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_TAIL_IDENTITY: usize = 88;
    /// Stated value of `zero_tail_identity` (`u32`). Spec §2.
    pub(crate) const ZERO_TAIL_IDENTITY_VALUE: u32 = 0x0000_0000;
    /// Offset of `one_tail_identity` (`u32`, little-endian). Spec §2.
    pub(crate) const ONE_TAIL_IDENTITY: usize = 92;
    /// Stated value of `one_tail_identity` (`u32`). Spec §2.
    pub(crate) const ONE_TAIL_IDENTITY_VALUE: u32 = 0x0000_0001;
}

/// Byte offsets for the `compact_legacy_84_construction_line` record.
///
/// Spec §2. Record length 84 B.
///
/// ```text
/// The endpoint fields are direct feature-local point-object identifiers.
/// ```
pub(crate) mod compact_legacy_84_construction_line {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 84;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `shared_selector` (`bytes[4]`). Spec §2.
    pub(crate) const SHARED_SELECTOR: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Stated value of `native_kind` (`u32`). Spec §2.
    pub(crate) const NATIVE_KIND_VALUE: u32 = 0x0000_0002;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0002;
    /// Offset of `state_at_29` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE_AT_29: usize = 29;
    /// Stated value of `state_at_29` (`u16`). Spec §2.
    pub(crate) const STATE_AT_29_VALUE: u16 = 0x0000;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `zero_endpoint_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX: usize = 60;
    /// Stated value of `zero_endpoint_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX_VALUE: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 64;
    /// Stated value of `signed_selector` (`f64`). Spec §2.
    pub(crate) const SIGNED_SELECTOR_VALUE: f64 = -1.0;
    /// Offset of `trailer_state` (`bytes[4]`). Spec §2.
    pub(crate) const TRAILER_STATE: usize = 72;
    /// Offset of `identity_first` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_FIRST: usize = 76;
    /// Offset of `identity_second` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_SECOND: usize = 80;
}

/// Byte offsets for the `compact_legacy_84_coordinate_roster_curve` record.
///
/// Spec §2. Record length 84 B.
///
/// ```text
/// The endpoint fields are zero-based ordinals in the complete feature-local coordinate-bearing marker roster.
/// ```
pub(crate) mod compact_legacy_84_coordinate_roster_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 84;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `shared_selector` (`bytes[4]`). Spec §2.
    pub(crate) const SHARED_SELECTOR: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `zero_endpoint_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX: usize = 60;
    /// Stated value of `zero_endpoint_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_ENDPOINT_PREFIX_VALUE: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 64;
    /// Stated value of `signed_selector` (`f64`). Spec §2.
    pub(crate) const SIGNED_SELECTOR_VALUE: f64 = -1.0;
    /// Offset of `trailer_state` (`bytes[4]`). Spec §2.
    pub(crate) const TRAILER_STATE: usize = 72;
    /// Offset of `identity_first` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_FIRST: usize = 76;
    /// Offset of `identity_second` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_SECOND: usize = 80;
}

/// Byte offsets for the `compact_legacy_84_geometry_indexed_curve` record.
///
/// Spec §2. Record length 84 B.
///
/// ```text
/// The endpoint fields are zero-based ordinals in the complete feature-local coordinate-bearing marker roster.
/// ```
pub(crate) mod compact_legacy_84_geometry_indexed_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 84;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `shared_selector` (`bytes[4]`). Spec §2.
    pub(crate) const SHARED_SELECTOR: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Stated value of `native_kind` (`u32`). Spec §2.
    pub(crate) const NATIVE_KIND_VALUE: u32 = 0x0000_0002;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0001;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Stated value of `state` (`u16`). Spec §2.
    pub(crate) const STATE_VALUE: u16 = 0x0002;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_scalar` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_SCALAR: usize = 48;
    /// Stated value of `state_scalar` (`f64`). Spec §2.
    pub(crate) const STATE_SCALAR_VALUE: f64 = 1.0;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 56;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 58;
    /// Offset of `record_state` (`u32`, little-endian). Spec §2.
    pub(crate) const RECORD_STATE: usize = 60;
    /// Stated value of `record_state` (`u32`). Spec §2.
    pub(crate) const RECORD_STATE_VALUE: u32 = 0x0000_0001;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 64;
    /// Stated value of `signed_selector` (`f64`). Spec §2.
    pub(crate) const SIGNED_SELECTOR_VALUE: f64 = -1.0;
    /// Offset of `trailer_state` (`u32`, little-endian). Spec §2.
    pub(crate) const TRAILER_STATE: usize = 72;
    /// Stated value of `trailer_state` (`u32`). Spec §2.
    pub(crate) const TRAILER_STATE_VALUE: u32 = 0x0000_0000;
    /// Offset of `identity_first` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_FIRST: usize = 76;
    /// Offset of `identity_second` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_SECOND: usize = 80;
}

/// Byte offsets for the `compact_legacy_68_profile_variant_curve` record.
///
/// Spec §2. Record length 68 B.
///
/// ```text
/// The role-1 profile-body variant uses u16 24 at +33 and carries two feature-local trailer object values.
/// ```
pub(crate) mod compact_legacy_68_profile_variant_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 68;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 13;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 19;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 23;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 25;
    /// Offset of `zero_body_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_BODY_PREFIX: usize = 27;
    /// Offset of `body_tag` (`u8`). Spec §2.
    pub(crate) const BODY_TAG: usize = 31;
    /// Offset of `body_zero` (`u8`). Spec §2.
    pub(crate) const BODY_ZERO: usize = 32;
    /// Offset of `profile_variant` (`u16`, little-endian). Spec §2.
    pub(crate) const PROFILE_VARIANT: usize = 33;
    /// Offset of `zero_body_suffix` (`bytes[7]`). Spec §2.
    pub(crate) const ZERO_BODY_SUFFIX: usize = 35;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 42;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 44;
    /// Offset of `selector_value` (`u32`, little-endian). Spec §2.
    pub(crate) const SELECTOR_VALUE: usize = 46;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 50;
    /// Offset of `tail_zero_first` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_ZERO_FIRST: usize = 58;
    /// Offset of `linked_object_first` (`u16`, little-endian). Spec §2.
    pub(crate) const LINKED_OBJECT_FIRST: usize = 60;
    /// Offset of `tail_zero_second` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_ZERO_SECOND: usize = 62;
    /// Offset of `linked_object_second` (`u16`, little-endian). Spec §2.
    pub(crate) const LINKED_OBJECT_SECOND: usize = 64;
    /// Offset of `tail_zero_third` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_ZERO_THIRD: usize = 66;
}

/// Byte offsets for the `compact_legacy_90_geometry_line` record.
///
/// Spec §2. Record length 90 B.
///
/// ```text
/// Endpoint values are zero-based indices in the feature-owned sketch-marker roster. The terminal variant extends the fixed body to marker +138 and has the terminal suffix described by the spec.
/// ```
pub(crate) mod compact_legacy_90_geometry_line {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 90;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 13;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 19;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 23;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 25;
    /// Offset of `body` (`bytes[11]`). Spec §2.
    pub(crate) const BODY: usize = 31;
    /// Offset of `endpoint_first` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_FIRST: usize = 42;
    /// Offset of `endpoint_second` (`u16`, little-endian). Spec §2.
    pub(crate) const ENDPOINT_SECOND: usize = 44;
    /// Offset of `selector_value` (`u32`, little-endian). Spec §2.
    pub(crate) const SELECTOR_VALUE: usize = 46;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 50;
    /// Offset of `tail_value` (`u32`, little-endian). Spec §2.
    pub(crate) const TAIL_VALUE: usize = 58;
    /// Offset of `sentinel_cells` (`i32[4]`, little-endian). Spec §2.
    pub(crate) const SENTINEL_CELLS: usize = 64;
    /// Offset of `tail_zero_suffix` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_ZERO_SUFFIX: usize = 80;
    /// Offset of `identity_first` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_FIRST: usize = 82;
    /// Offset of `identity_second` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_SECOND: usize = 86;
}

/// Byte offsets for the `compact_legacy_142_profile_curve` record.
///
/// Spec §2. Record length 142 B.
///
/// ```text
/// The auxiliary pair is the arc-center candidate. Equal positive endpoint radii select a minor arc; otherwise the two endpoint pairs define a line. A four-byte separator may follow the 142-byte body before the next sketch marker.
/// ```
pub(crate) mod compact_legacy_142_profile_curve {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 142;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `shared_selector` (`f32`, little-endian). Spec §2.
    pub(crate) const SHARED_SELECTOR: usize = 13;
    /// Stated value of `shared_selector` (`f32`). Spec §2.
    pub(crate) const SHARED_SELECTOR_VALUE: f32 = -1.0;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Stated value of `native_kind` (`u32`). Spec §2.
    pub(crate) const NATIVE_KIND_VALUE: u32 = 0x0000_0002;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0001;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `curve_tag` (`bytes[2]`). Spec §2.
    pub(crate) const CURVE_TAG: usize = 64;
    /// Offset of `auxiliary_first` (`f64`, little-endian). Spec §2.
    pub(crate) const AUXILIARY_FIRST: usize = 66;
    /// Offset of `auxiliary_second` (`f64`, little-endian). Spec §2.
    pub(crate) const AUXILIARY_SECOND: usize = 74;
    /// Offset of `body_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const BODY_KIND: usize = 82;
    /// Stated value of `body_kind` (`u32`). Spec §2.
    pub(crate) const BODY_KIND_VALUE: u32 = 0x0000_000b;
    /// Offset of `variant` (`u32`, little-endian). Spec §2.
    pub(crate) const VARIANT: usize = 92;
    /// Offset of `start_first` (`f64`, little-endian). Spec §2.
    pub(crate) const START_FIRST: usize = 96;
    /// Offset of `start_second` (`f64`, little-endian). Spec §2.
    pub(crate) const START_SECOND: usize = 104;
    /// Offset of `end_first` (`f64`, little-endian). Spec §2.
    pub(crate) const END_FIRST: usize = 112;
    /// Offset of `end_second` (`f64`, little-endian). Spec §2.
    pub(crate) const END_SECOND: usize = 120;
    /// Offset of `identity` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY: usize = 138;
}

/// Byte offsets for the `compact_legacy_code_two_profile_point` record.
///
/// Spec §2. Record length 132 B.
///
/// ```text
/// The record emits a point and contributes its coordinate to the raw coordinate roster.
/// ```
pub(crate) mod compact_legacy_code_two_profile_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 132;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 13;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 19;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 23;
    /// Offset of `zero_state` (`bytes[6]`). Spec §2.
    pub(crate) const ZERO_STATE: usize = 25;
    /// Offset of `selector` (`bytes[11]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 42;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 44;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 52;
    /// Offset of `zero_link_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_LINK_PREFIX: usize = 60;
    /// Offset of `operand_tag` (`u16`, little-endian). Spec §2.
    pub(crate) const OPERAND_TAG: usize = 62;
    /// Offset of `operand_first` (`bytes[8]`). Spec §2.
    pub(crate) const OPERAND_FIRST: usize = 64;
    /// Offset of `operand_second` (`bytes[8]`). Spec §2.
    pub(crate) const OPERAND_SECOND: usize = 72;
    /// Offset of `link_terminator` (`bytes[6]`). Spec §2.
    pub(crate) const LINK_TERMINATOR: usize = 80;
    /// Offset of `zero_trailer` (`bytes[34]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 86;
    /// Offset of `trailer_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const TRAILER_KIND: usize = 120;
    /// Offset of `zero_identity_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_IDENTITY_PREFIX: usize = 124;
    /// Offset of `identity` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY: usize = 128;
}

/// Byte offsets for the `compact_legacy_embedded_geometry_handle` record.
///
/// Spec §2. Record length 120 B.
///
/// ```text
/// The record contributes its coordinate to the raw coordinate roster and emits no sketch entity.
/// ```
pub(crate) mod compact_legacy_embedded_geometry_handle {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 120;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 13;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 17;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 19;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 23;
    /// Offset of `zero_state_prefix` (`bytes[6]`). Spec §2.
    pub(crate) const ZERO_STATE_PREFIX: usize = 25;
    /// Offset of `selector` (`bytes[11]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 42;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 44;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 52;
    /// Offset of `state` (`u32`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 60;
    /// Offset of `zero_link_prefix` (`bytes[6]`). Spec §2.
    pub(crate) const ZERO_LINK_PREFIX: usize = 64;
    /// Offset of `link_sentinel` (`i32`, little-endian). Spec §2.
    pub(crate) const LINK_SENTINEL: usize = 70;
    /// Offset of `zero_trailer` (`bytes[42]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 74;
    /// Offset of `identity` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY: usize = 116;
}

/// Byte offsets for the `compact_legacy_terminal_diameter_circle` record.
///
/// Spec §2. Record length 121 B.
///
/// ```text
/// The radial ordinal is zero-based in the feature-owned raw coordinate roster, including coordinate-bearing geometry handles that do not emit sketch entities.
/// ```
pub(crate) mod compact_legacy_terminal_diameter_circle {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 121;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 13;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 17;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 19;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 23;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 25;
    /// Offset of `zero_selector_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_SELECTOR_PREFIX: usize = 27;
    /// Offset of `selector` (`bytes[11]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `radial_ordinal` (`u16`, little-endian). Spec §2.
    pub(crate) const RADIAL_ORDINAL: usize = 42;
    /// Offset of `radial_sentinel` (`u16`, little-endian). Spec §2.
    pub(crate) const RADIAL_SENTINEL: usize = 44;
    /// Offset of `selector_value` (`u32`, little-endian). Spec §2.
    pub(crate) const SELECTOR_VALUE: usize = 46;
    /// Offset of `signed_selector` (`f64`, little-endian). Spec §2.
    pub(crate) const SIGNED_SELECTOR: usize = 50;
    /// Offset of `zero_trailer` (`bytes[44]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 58;
    /// Offset of `terminal_state` (`u16`, little-endian). Spec §2.
    pub(crate) const TERMINAL_STATE: usize = 102;
    /// Offset of `class_marker` (`bytes[4]`). Spec §2.
    pub(crate) const CLASS_MARKER: usize = 104;
    /// Offset of `class_length` (`u16`, little-endian). Spec §2.
    pub(crate) const CLASS_LENGTH: usize = 108;
    /// Offset of `class_name` (`bytes[11]`). Spec §2.
    pub(crate) const CLASS_NAME: usize = 110;
}

/// Byte offsets for the `legacy_geometry_locus_alternate_170_line_handle_point` record.
///
/// Spec §2. Record length 170 B.
///
/// ```text
/// The line-handle declaration makes the coordinate-bearing marker a point; the next sketch marker begins at +170.
/// ```
pub(crate) mod legacy_geometry_locus_alternate_170_line_handle_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 170;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 21;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state` (`u16`, little-endian). Spec §2.
    pub(crate) const STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `zero_before_state_value` (`bytes[9]`). Spec §2.
    pub(crate) const ZERO_BEFORE_STATE_VALUE: usize = 39;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 58;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 66;
    /// Offset of `zero_handle_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_HANDLE_PREFIX: usize = 74;
    /// Offset of `handle_state` (`u16`, little-endian). Spec §2.
    pub(crate) const HANDLE_STATE: usize = 76;
    /// Offset of `class_marker_and_length` (`bytes[6]`). Spec §2.
    pub(crate) const CLASS_MARKER_AND_LENGTH: usize = 78;
    /// Offset of `class_name` (`bytes[12]`). Spec §2.
    pub(crate) const CLASS_NAME: usize = 84;
    /// Offset of `handle_identifier` (`u16`, little-endian). Spec §2.
    pub(crate) const HANDLE_IDENTIFIER: usize = 96;
    /// Offset of `reference_tail` (`bytes[8]`). Spec §2.
    pub(crate) const REFERENCE_TAIL: usize = 98;
    /// Offset of `zero_before_sentinel` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_BEFORE_SENTINEL: usize = 106;
    /// Offset of `reference_sentinel` (`bytes[4]`). Spec §2.
    pub(crate) const REFERENCE_SENTINEL: usize = 110;
    /// Offset of `reference_zero_tail` (`bytes[4]`). Spec §2.
    pub(crate) const REFERENCE_ZERO_TAIL: usize = 114;
    /// Offset of `terminator` (`bytes[6]`). Spec §2.
    pub(crate) const TERMINATOR: usize = 118;
    /// Offset of `zero_trailer_prefix` (`bytes[38]`). Spec §2.
    pub(crate) const ZERO_TRAILER_PREFIX: usize = 124;
    /// Offset of `identity` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY: usize = 162;
    /// Offset of `following_object_index` (`u32`, little-endian). Spec §2.
    pub(crate) const FOLLOWING_OBJECT_INDEX: usize = 166;
}

/// Byte offsets for the `legacy_geometry_locus_alternate_169_arc_handle_point` record.
///
/// Spec §2. Record length 169 B.
///
/// ```text
/// The arc-handle declaration makes the coordinate-bearing marker a point; the next sketch marker begins at +169.
/// ```
pub(crate) mod legacy_geometry_locus_alternate_169_arc_handle_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 169;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `zero_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_PREFIX: usize = 21;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Offset of `state_before_handle` (`bytes[2]`). Spec §2.
    pub(crate) const STATE_BEFORE_HANDLE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `zero_before_state_value` (`bytes[9]`). Spec §2.
    pub(crate) const ZERO_BEFORE_STATE_VALUE: usize = 39;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 58;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 66;
    /// Offset of `zero_handle_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_HANDLE_PREFIX: usize = 74;
    /// Offset of `handle_state` (`u16`, little-endian). Spec §2.
    pub(crate) const HANDLE_STATE: usize = 76;
    /// Stated value of `handle_state` (`u16`). Spec §2.
    pub(crate) const HANDLE_STATE_VALUE: u16 = 0x0002;
    /// Offset of `reference_cell` (`bytes[4]`). Spec §2.
    pub(crate) const REFERENCE_CELL: usize = 78;
    /// Offset of `reference_tail` (`bytes[8]`). Spec §2.
    pub(crate) const REFERENCE_TAIL: usize = 82;
    /// Offset of `class_marker_and_length` (`bytes[6]`). Spec §2.
    pub(crate) const CLASS_MARKER_AND_LENGTH: usize = 90;
    /// Offset of `class_name` (`bytes[11]`). Spec §2.
    pub(crate) const CLASS_NAME: usize = 96;
    /// Offset of `handle_identifier` (`u16`, little-endian). Spec §2.
    pub(crate) const HANDLE_IDENTIFIER: usize = 107;
    /// Stated value of `handle_identifier` (`u16`). Spec §2.
    pub(crate) const HANDLE_IDENTIFIER_VALUE: u16 = 0x0000;
    /// Offset of `reference_sentinel_and_zero_tail` (`bytes[8]`). Spec §2.
    pub(crate) const REFERENCE_SENTINEL_AND_ZERO_TAIL: usize = 109;
    /// Offset of `terminator` (`bytes[6]`). Spec §2.
    pub(crate) const TERMINATOR: usize = 117;
    /// Offset of `zero_trailer` (`bytes[42]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 123;
    /// Offset of `identity` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY: usize = 165;
}

/// Byte offsets for the `current_geometry_locus_arc_handle_point` record.
///
/// Spec §2. Record length 167 B.
///
/// ```text
/// The record includes the following marker's object index at +163; the following marker begins at +167.
/// ```
pub(crate) mod current_geometry_locus_arc_handle_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 167;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `shared_selector` (`bytes[4]`). Spec §2.
    pub(crate) const SHARED_SELECTOR: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Stated value of `native_kind` (`u32`). Spec §2.
    pub(crate) const NATIVE_KIND_VALUE: u32 = 0x0000_0000;
    /// Offset of `zero_locus_prefix` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_LOCUS_PREFIX: usize = 21;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0001;
    /// Offset of `zero_state` (`bytes[2]`). Spec §2.
    pub(crate) const ZERO_STATE: usize = 29;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `zero_before_state_value` (`bytes[9]`). Spec §2.
    pub(crate) const ZERO_BEFORE_STATE_VALUE: usize = 39;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `zero_before_coordinate` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_BEFORE_COORDINATE: usize = 56;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 64;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 66;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 74;
    /// Offset of `handle_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const HANDLE_PREFIX: usize = 82;
    /// Offset of `class_marker` (`bytes[4]`). Spec §2.
    pub(crate) const CLASS_MARKER: usize = 86;
    /// Offset of `class_length` (`u16`, little-endian). Spec §2.
    pub(crate) const CLASS_LENGTH: usize = 90;
    /// Stated value of `class_length` (`u16`). Spec §2.
    pub(crate) const CLASS_LENGTH_VALUE: u16 = 0x000b;
    /// Offset of `class_name` (`bytes[11]`). Spec §2.
    pub(crate) const CLASS_NAME: usize = 92;
    /// Offset of `handle_id` (`u16`, little-endian). Spec §2.
    pub(crate) const HANDLE_ID: usize = 103;
    /// Offset of `reference_sentinel` (`bytes[4]`). Spec §2.
    pub(crate) const REFERENCE_SENTINEL: usize = 105;
    /// Offset of `zero_reference_tail` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_REFERENCE_TAIL: usize = 109;
    /// Offset of `terminator` (`bytes[4]`). Spec §2.
    pub(crate) const TERMINATOR: usize = 117;
    /// Offset of `zero_trailer` (`bytes[42]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 121;
    /// Offset of `following_object_index` (`u32`, little-endian). Spec §2.
    pub(crate) const FOLLOWING_OBJECT_INDEX: usize = 163;
}

/// Byte offsets for the `current_geometry_locus_arc_handle_point_terminal` record.
///
/// Spec §2. Record length 171 B.
///
/// ```text
/// The record includes a four-byte zero separator at +163, the following marker's object index at +167, and the following marker begins at +171.
/// ```
pub(crate) mod current_geometry_locus_arc_handle_point_terminal {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 171;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `shared_selector` (`bytes[4]`). Spec §2.
    pub(crate) const SHARED_SELECTOR: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Stated value of `native_kind` (`u32`). Spec §2.
    pub(crate) const NATIVE_KIND_VALUE: u32 = 0x0000_0000;
    /// Offset of `geometry_locus` (`bytes[4]`). Spec §2.
    pub(crate) const GEOMETRY_LOCUS: usize = 23;
    /// Offset of `role` (`u16`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 27;
    /// Stated value of `role` (`u16`). Spec §2.
    pub(crate) const ROLE_VALUE: u16 = 0x0001;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Stated value of `state_value` (`f64`). Spec §2.
    pub(crate) const STATE_VALUE_VALUE: f64 = 1.0;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 64;
    /// Offset of `coordinate_first` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_FIRST: usize = 66;
    /// Offset of `coordinate_second` (`f64`, little-endian). Spec §2.
    pub(crate) const COORDINATE_SECOND: usize = 74;
    /// Offset of `handle_prefix` (`bytes[4]`). Spec §2.
    pub(crate) const HANDLE_PREFIX: usize = 82;
    /// Offset of `class_marker` (`bytes[4]`). Spec §2.
    pub(crate) const CLASS_MARKER: usize = 86;
    /// Offset of `class_length` (`u16`, little-endian). Spec §2.
    pub(crate) const CLASS_LENGTH: usize = 90;
    /// Stated value of `class_length` (`u16`). Spec §2.
    pub(crate) const CLASS_LENGTH_VALUE: u16 = 0x000b;
    /// Offset of `class_name` (`bytes[11]`). Spec §2.
    pub(crate) const CLASS_NAME: usize = 92;
    /// Offset of `handle_id` (`u16`, little-endian). Spec §2.
    pub(crate) const HANDLE_ID: usize = 103;
    /// Offset of `reference_sentinel` (`bytes[4]`). Spec §2.
    pub(crate) const REFERENCE_SENTINEL: usize = 105;
    /// Offset of `zero_reference_tail` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_REFERENCE_TAIL: usize = 109;
    /// Offset of `terminator` (`bytes[4]`). Spec §2.
    pub(crate) const TERMINATOR: usize = 117;
    /// Offset of `zero_trailer` (`bytes[46]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 121;
    /// Offset of `following_object_index` (`u32`, little-endian). Spec §2.
    pub(crate) const FOLLOWING_OBJECT_INDEX: usize = 167;
}

/// Byte offsets for the `reference_point_short_solved_cache` record.
///
/// Spec §2. Record length 277 B.
///
/// ```text
/// Offsets begin at the byte after the UTF-16LE feature name. Unlisted bytes belong to the native construction state.
/// ```
pub(crate) mod reference_point_short_solved_cache {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 277;
    /// Offset of `object_header` (`bytes[8]`). Spec §2.
    pub(crate) const OBJECT_HEADER: usize = 0;
    /// Offset of `object_id` (`u32`, little-endian). Spec §2.
    pub(crate) const OBJECT_ID: usize = 8;
    /// Offset of `zero_after_id` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_AFTER_ID: usize = 12;
    /// Offset of `zero_before_position` (`bytes[16]`). Spec §2.
    pub(crate) const ZERO_BEFORE_POSITION: usize = 227;
    /// Offset of `position` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const POSITION: usize = 243;
    /// Offset of `construction_form` (`u16`, little-endian). Spec §2.
    pub(crate) const CONSTRUCTION_FORM: usize = 267;
    /// Offset of `zero_trailer` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 269;
}

/// Byte offsets for the `reference_point_long_solved_cache` record.
///
/// Spec §2. Record length 293 B.
///
/// ```text
/// Offsets begin at the byte after the UTF-16LE feature name. Unlisted bytes belong to the native construction state.
/// ```
pub(crate) mod reference_point_long_solved_cache {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 293;
    /// Offset of `object_header` (`bytes[8]`). Spec §2.
    pub(crate) const OBJECT_HEADER: usize = 0;
    /// Offset of `object_id` (`u32`, little-endian). Spec §2.
    pub(crate) const OBJECT_ID: usize = 8;
    /// Offset of `zero_after_id` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_AFTER_ID: usize = 12;
    /// Offset of `zero_before_position` (`bytes[16]`). Spec §2.
    pub(crate) const ZERO_BEFORE_POSITION: usize = 243;
    /// Offset of `position` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const POSITION: usize = 259;
    /// Offset of `construction_form` (`u16`, little-endian). Spec §2.
    pub(crate) const CONSTRUCTION_FORM: usize = 283;
    /// Offset of `zero_trailer` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_TRAILER: usize = 285;
}

/// Byte offsets for the `extrusion_sparse_operation_trailer` record.
///
/// Spec §2. Record length 40 B.
pub(crate) mod extrusion_sparse_operation_trailer {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 40;
    /// Offset of `zero_header` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_HEADER: usize = 0;
    /// Offset of `family` (`u16`, little-endian). Spec §2.
    pub(crate) const FAMILY: usize = 4;
    /// Offset of `operation` (`u8`). Spec §2.
    pub(crate) const OPERATION: usize = 6;
    /// Offset of `schema` (`u8`). Spec §2.
    pub(crate) const SCHEMA: usize = 7;
    /// Offset of `object_id` (`u32`, little-endian). Spec §2.
    pub(crate) const OBJECT_ID: usize = 8;
    /// Offset of `zero_after_object` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_AFTER_OBJECT: usize = 12;
    /// Offset of `sparse_zero_prefix` (`bytes[6]`). Spec §2.
    pub(crate) const SPARSE_ZERO_PREFIX: usize = 16;
    /// Offset of `sparse_marker` (`u16`, little-endian). Spec §2.
    pub(crate) const SPARSE_MARKER: usize = 22;
    /// Offset of `first_token` (`u16`, little-endian). Spec §2.
    pub(crate) const FIRST_TOKEN: usize = 24;
    /// Offset of `optional_identity` (`u32`, little-endian). Spec §2.
    pub(crate) const OPTIONAL_IDENTITY: usize = 26;
    /// Offset of `zero_before_final_token` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_BEFORE_FINAL_TOKEN: usize = 30;
    /// Offset of `final_token` (`u16`, little-endian). Spec §2.
    pub(crate) const FINAL_TOKEN: usize = 38;
}

/// Byte offsets for the `coordinate_system_component_point` record.
///
/// Spec §2. Record length 151 B.
pub(crate) mod coordinate_system_component_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 151;
    /// Offset of `prefix` (`bytes[10]`). Spec §2.
    pub(crate) const PREFIX: usize = 0;
    /// Offset of `zero_header` (`bytes[35]`). Spec §2.
    pub(crate) const ZERO_HEADER: usize = 10;
    /// Offset of `sentinel` (`bytes[16]`). Spec §2.
    pub(crate) const SENTINEL: usize = 45;
    /// Offset of `zero_before_source` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_BEFORE_SOURCE: usize = 61;
    /// Offset of `source_id` (`u32`, little-endian). Spec §2.
    pub(crate) const SOURCE_ID: usize = 69;
    /// Offset of `source_stamp` (`u32`, little-endian). Spec §2.
    pub(crate) const SOURCE_STAMP: usize = 73;
    /// Offset of `zero_selector` (`u16`, little-endian). Spec §2.
    pub(crate) const ZERO_SELECTOR: usize = 77;
    /// Offset of `one_selector` (`u16`, little-endian). Spec §2.
    pub(crate) const ONE_SELECTOR: usize = 79;
    /// Offset of `zero_before_object` (`bytes[6]`). Spec §2.
    pub(crate) const ZERO_BEFORE_OBJECT: usize = 81;
    /// Offset of `object_id` (`u32`, little-endian). Spec §2.
    pub(crate) const OBJECT_ID: usize = 87;
    /// Offset of `zero_before_handles` (`bytes[12]`). Spec §2.
    pub(crate) const ZERO_BEFORE_HANDLES: usize = 91;
    /// Offset of `handles` (`bytes[8]`). Spec §2.
    pub(crate) const HANDLES: usize = 103;
    /// Offset of `zero_before_generation` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_GENERATION: usize = 111;
    /// Offset of `generation` (`u32`, little-endian). Spec §2.
    pub(crate) const GENERATION: usize = 115;
    /// Offset of `zero_before_origin` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_BEFORE_ORIGIN: usize = 119;
    /// Offset of `origin` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const ORIGIN: usize = 127;
}

/// Byte offsets for the `coordinate_system_extended_component_point` record.
///
/// Spec §2. Record length 165 B.
pub(crate) mod coordinate_system_extended_component_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 165;
    /// Offset of `prefix_and_source` (`bytes[77]`). Spec §2.
    pub(crate) const PREFIX_AND_SOURCE: usize = 0;
    /// Offset of `reference_id` (`u32`, little-endian). Spec §2.
    pub(crate) const REFERENCE_ID: usize = 77;
    /// Offset of `sentinel` (`u32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 81;
    /// Offset of `zero_before_count` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_COUNT: usize = 85;
    /// Offset of `reference_count` (`u32`, little-endian). Spec §2.
    pub(crate) const REFERENCE_COUNT: usize = 89;
    /// Offset of `one` (`u32`, little-endian). Spec §2.
    pub(crate) const ONE: usize = 93;
    /// Offset of `zero_before_object` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_OBJECT: usize = 97;
    /// Offset of `object_id` (`u32`, little-endian). Spec §2.
    pub(crate) const OBJECT_ID: usize = 101;
    /// Offset of `zero_before_handles` (`bytes[12]`). Spec §2.
    pub(crate) const ZERO_BEFORE_HANDLES: usize = 105;
    /// Offset of `handles` (`bytes[8]`). Spec §2.
    pub(crate) const HANDLES: usize = 117;
    /// Offset of `zero_before_generation` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_GENERATION: usize = 125;
    /// Offset of `generation` (`u32`, little-endian). Spec §2.
    pub(crate) const GENERATION: usize = 129;
    /// Offset of `zero_before_origin` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_BEFORE_ORIGIN: usize = 133;
    /// Offset of `origin` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const ORIGIN: usize = 141;
}

/// Byte offsets for the `coordinate_system_component_path_prefix` record.
///
/// Spec §2. Record length 110 B.
///
/// ```text
/// The counted compact component path starts immediately after this prefix. Its byte length depends on its typed entries and separators.
/// ```
pub(crate) mod coordinate_system_component_path_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 110;
    /// Offset of `common_prefix_and_source` (`bytes[73]`). Spec §2.
    pub(crate) const COMMON_PREFIX_AND_SOURCE: usize = 0;
    /// Offset of `sentinel` (`bytes[7]`). Spec §2.
    pub(crate) const SENTINEL: usize = 73;
    /// Offset of `path_entry_count` (`u32`, little-endian). Spec §2.
    pub(crate) const PATH_ENTRY_COUNT: usize = 80;
    /// Offset of `path_kind` (`bytes[4]`). Spec §2.
    pub(crate) const PATH_KIND: usize = 84;
    /// Offset of `zero_before_marker` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_MARKER: usize = 88;
    /// Offset of `component_marker` (`bytes[16]`). Spec §2.
    pub(crate) const COMPONENT_MARKER: usize = 92;
    /// Offset of `zero_before_path` (`u16`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_PATH: usize = 108;
}

/// Byte offsets for the `coordinate_system_component_path_suffix` record.
///
/// Spec §2. Record length 86 B.
///
/// ```text
/// Path-end-relative. An optional eight-byte terminal null slot precedes this suffix and is not part of its size.
/// ```
pub(crate) mod coordinate_system_component_path_suffix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 86;
    /// Offset of `zero_header` (`bytes[14]`). Spec §2.
    pub(crate) const ZERO_HEADER: usize = 0;
    /// Offset of `one` (`u32`, little-endian). Spec §2.
    pub(crate) const ONE: usize = 14;
    /// Offset of `zero_before_object` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_OBJECT: usize = 18;
    /// Offset of `object_id` (`u32`, little-endian). Spec §2.
    pub(crate) const OBJECT_ID: usize = 22;
    /// Offset of `zero_before_handles` (`bytes[12]`). Spec §2.
    pub(crate) const ZERO_BEFORE_HANDLES: usize = 26;
    /// Offset of `handles` (`bytes[8]`). Spec §2.
    pub(crate) const HANDLES: usize = 38;
    /// Offset of `zero_before_generation` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_GENERATION: usize = 46;
    /// Offset of `generation` (`u32`, little-endian). Spec §2.
    pub(crate) const GENERATION: usize = 50;
    /// Offset of `zero_before_origin` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_BEFORE_ORIGIN: usize = 54;
    /// Offset of `origin` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const ORIGIN: usize = 62;
}

/// Byte offsets for the `coordinate_system_ordinal_axis_tail` record.
///
/// Spec §2. Record length 35 B.
///
/// ```text
/// Origin-end-relative. One or two nonzero u16 tokens follow this fixed core and terminate the feature object.
/// ```
pub(crate) mod coordinate_system_ordinal_axis_tail {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 35;
    /// Offset of `x_axis_ordinal` (`u16`, little-endian). Spec §2.
    pub(crate) const X_AXIS_ORDINAL: usize = 0;
    /// Offset of `y_axis_ordinal` (`u16`, little-endian). Spec §2.
    pub(crate) const Y_AXIS_ORDINAL: usize = 2;
    /// Offset of `zero_before_origin_z` (`bytes[23]`). Spec §2.
    pub(crate) const ZERO_BEFORE_ORIGIN_Z: usize = 4;
    /// Offset of `origin_z` (`f64`, little-endian). Spec §2.
    pub(crate) const ORIGIN_Z: usize = 27;
}

/// Byte offsets for the `coordinate_system_two_point_separator` record.
///
/// Spec §2. Record length 14 B.
pub(crate) mod coordinate_system_two_point_separator {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 14;
    /// Offset of `selectors` (`u16[3]`, little-endian). Spec §2.
    pub(crate) const SELECTORS: usize = 0;
    /// Offset of `first_token` (`u16`, little-endian). Spec §2.
    pub(crate) const FIRST_TOKEN: usize = 6;
    /// Offset of `one` (`u16`, little-endian). Spec §2.
    pub(crate) const ONE: usize = 8;
    /// Offset of `final_tokens` (`u16[2]`, little-endian). Spec §2.
    pub(crate) const FINAL_TOKENS: usize = 10;
}

/// Byte offsets for the `coordinate_system_two_point_tail` record.
///
/// Spec §2. Record length 94 B.
pub(crate) mod coordinate_system_two_point_tail {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 94;
    /// Offset of `origin_yz` (`f64[2]`, little-endian). Spec §2.
    pub(crate) const ORIGIN_YZ: usize = 0;
    /// Offset of `x_direction` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const X_DIRECTION: usize = 16;
    /// Offset of `separator` (`u8`). Spec §2.
    pub(crate) const SEPARATOR: usize = 40;
    /// Offset of `repeated_x_direction` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const REPEATED_X_DIRECTION: usize = 41;
    /// Offset of `zero_before_origin` (`bytes[3]`). Spec §2.
    pub(crate) const ZERO_BEFORE_ORIGIN: usize = 65;
    /// Offset of `origin` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const ORIGIN: usize = 68;
    /// Offset of `terminal_token` (`u16`, little-endian). Spec §2.
    pub(crate) const TERMINAL_TOKEN: usize = 92;
}

/// Byte offsets for the `coordinate_system_endpoint_path_prefix` record.
///
/// Spec §2. Record length 110 B.
///
/// ```text
/// The counted compact component path starts immediately after this fixed prefix.
/// ```
pub(crate) mod coordinate_system_endpoint_path_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 110;
    /// Offset of `family` (`bytes[17]`). Spec §2.
    pub(crate) const FAMILY: usize = 0;
    /// Offset of `zero_header` (`bytes[28]`). Spec §2.
    pub(crate) const ZERO_HEADER: usize = 17;
    /// Offset of `sentinel` (`bytes[16]`). Spec §2.
    pub(crate) const SENTINEL: usize = 45;
    /// Offset of `zero_before_selector` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_BEFORE_SELECTOR: usize = 61;
    /// Offset of `selector` (`u32`, little-endian). Spec §2.
    pub(crate) const SELECTOR: usize = 69;
    /// Offset of `zero_before_count` (`bytes[7]`). Spec §2.
    pub(crate) const ZERO_BEFORE_COUNT: usize = 73;
    /// Offset of `path_entry_count` (`u32`, little-endian). Spec §2.
    pub(crate) const PATH_ENTRY_COUNT: usize = 80;
    /// Offset of `path_kind` (`bytes[4]`). Spec §2.
    pub(crate) const PATH_KIND: usize = 84;
    /// Offset of `token` (`u32`, little-endian). Spec §2.
    pub(crate) const TOKEN: usize = 88;
    /// Offset of `component_marker` (`bytes[16]`). Spec §2.
    pub(crate) const COMPONENT_MARKER: usize = 92;
    /// Offset of `zero_before_path` (`u16`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_PATH: usize = 108;
}

/// Byte offsets for the `coordinate_system_endpoint_path_suffix` record.
///
/// Spec §2. Record length 142 B.
///
/// ```text
/// Path-end-relative after the required eight-byte null slot.
/// ```
pub(crate) mod coordinate_system_endpoint_path_suffix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 142;
    /// Offset of `zero_header` (`bytes[70]`). Spec §2.
    pub(crate) const ZERO_HEADER: usize = 0;
    /// Offset of `one` (`u32`, little-endian). Spec §2.
    pub(crate) const ONE: usize = 70;
    /// Offset of `zero_before_object` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_OBJECT: usize = 74;
    /// Offset of `object_id` (`u32`, little-endian). Spec §2.
    pub(crate) const OBJECT_ID: usize = 78;
    /// Offset of `zero_before_handles` (`bytes[12]`). Spec §2.
    pub(crate) const ZERO_BEFORE_HANDLES: usize = 82;
    /// Offset of `handles` (`bytes[8]`). Spec §2.
    pub(crate) const HANDLES: usize = 94;
    /// Offset of `zero_before_generation` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_GENERATION: usize = 102;
    /// Offset of `generation` (`u32`, little-endian). Spec §2.
    pub(crate) const GENERATION: usize = 106;
    /// Offset of `zero_before_origin` (`bytes[8]`). Spec §2.
    pub(crate) const ZERO_BEFORE_ORIGIN: usize = 110;
    /// Offset of `origin` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const ORIGIN: usize = 118;
}

/// Byte offsets for the `coordinate_system_line_axis` record.
///
/// Spec §2. Record length 113 B.
pub(crate) mod coordinate_system_line_axis {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 113;
    /// Offset of `handles` (`bytes[8]`). Spec §2.
    pub(crate) const HANDLES: usize = 0;
    /// Offset of `zero_before_generation` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_BEFORE_GENERATION: usize = 8;
    /// Offset of `generation` (`u32`, little-endian). Spec §2.
    pub(crate) const GENERATION: usize = 12;
    /// Offset of `zero_before_scalar` (`bytes[16]`). Spec §2.
    pub(crate) const ZERO_BEFORE_SCALAR: usize = 16;
    /// Offset of `carrier_scalar` (`f64`, little-endian). Spec §2.
    pub(crate) const CARRIER_SCALAR: usize = 32;
    /// Offset of `line_point` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const LINE_POINT: usize = 40;
    /// Offset of `direction` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const DIRECTION: usize = 64;
    /// Offset of `separator` (`u8`). Spec §2.
    pub(crate) const SEPARATOR: usize = 88;
    /// Offset of `repeated_direction` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const REPEATED_DIRECTION: usize = 89;
}

/// Byte offsets for the `coordinate_system_xy_tail` record.
///
/// Spec §2. Record length 29 B.
pub(crate) mod coordinate_system_xy_tail {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 29;
    /// Offset of `x_reversed` (`u8`). Spec §2.
    pub(crate) const X_REVERSED: usize = 0;
    /// Offset of `y_reversed` (`u8`). Spec §2.
    pub(crate) const Y_REVERSED: usize = 1;
    /// Offset of `z_reversed` (`u8`). Spec §2.
    pub(crate) const Z_REVERSED: usize = 2;
    /// Offset of `origin` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const ORIGIN: usize = 3;
    /// Offset of `terminator` (`u16`, little-endian). Spec §2.
    pub(crate) const TERMINATOR: usize = 27;
}

/// Byte offsets for the `constructed_reference_plane_fixed_frame` record.
///
/// Spec §2. Record length 97 B.
///
/// ```text
/// Offsets begin immediately after the data-class name. The pairwise-orthogonal form uses both basis triples; the `moFixedRefPlnData_c` repeated-normal form uses one in-plane triple and duplicates the normal in the other. A valid 121-byte matrix frame at the same offset owns this 97-byte prefix.
/// ```
pub(crate) mod constructed_reference_plane_fixed_frame {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 97;
    /// Offset of `origin` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const ORIGIN: usize = 0;
    /// Offset of `normal` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const NORMAL: usize = 24;
    /// Offset of `frame_marker` (`u8`). Spec §2.
    pub(crate) const FRAME_MARKER: usize = 48;
    /// Offset of `u_axis` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const U_AXIS: usize = 49;
    /// Offset of `v_axis` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const V_AXIS: usize = 73;
}

/// Byte offsets for the `constructed_reference_plane_matrix_frame` record.
///
/// Spec §2. Record length 121 B.
///
/// ```text
/// Offsets begin immediately after the `moConstraintCoincLineAtAnglePlaneRefplaneData_c` class name.
/// ```
pub(crate) mod constructed_reference_plane_matrix_frame {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 121;
    /// Offset of `origin` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const ORIGIN: usize = 0;
    /// Offset of `normal` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const NORMAL: usize = 24;
    /// Offset of `frame_marker` (`u8`). Spec §2.
    pub(crate) const FRAME_MARKER: usize = 48;
    /// Offset of `basis_matrix` (`f64[9]`, little-endian). Spec §2.
    pub(crate) const BASIS_MATRIX: usize = 49;
}

/// Byte offsets for the `component_face_nested_reference_prefix` record.
///
/// Spec §2. Record length 102 B.
///
/// ```text
/// Offsets begin at the `moCompFace_c` body. The nested class declaration is variable within the fixed region; the component-path entries follow the marker tail.
/// ```
pub(crate) mod component_face_nested_reference_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 102;
    /// Offset of `class_token` (`u16`, little-endian). Spec §2.
    pub(crate) const CLASS_TOKEN: usize = 0;
    /// Offset of `record_version` (`u32`, little-endian). Spec §2.
    pub(crate) const RECORD_VERSION: usize = 2;
    /// Offset of `flags` (`bytes[2]`). Spec §2.
    pub(crate) const FLAGS: usize = 6;
    /// Offset of `component_marker` (`bytes[16]`). Spec §2.
    pub(crate) const COMPONENT_MARKER: usize = 84;
    /// Offset of `marker_tail` (`u16`, little-endian). Spec §2.
    pub(crate) const MARKER_TAIL: usize = 100;
}

/// Byte offsets for the `component_face_compact_reference_prefix` record.
///
/// Spec §2. Record length 82 B.
///
/// ```text
/// Offsets begin at the `moCompFace_c` body. The component-path entries follow the marker tail.
/// ```
pub(crate) mod component_face_compact_reference_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 82;
    /// Offset of `class_token` (`u16`, little-endian). Spec §2.
    pub(crate) const CLASS_TOKEN: usize = 0;
    /// Offset of `record_version` (`u32`, little-endian). Spec §2.
    pub(crate) const RECORD_VERSION: usize = 2;
    /// Offset of `flags` (`bytes[2]`). Spec §2.
    pub(crate) const FLAGS: usize = 6;
    /// Offset of `component_marker` (`bytes[16]`). Spec §2.
    pub(crate) const COMPONENT_MARKER: usize = 64;
    /// Offset of `marker_tail` (`u16`, little-endian). Spec §2.
    pub(crate) const MARKER_TAIL: usize = 80;
}

/// Byte offsets for the `component_face_flagged_operation_prefix` record.
///
/// Spec §2. Record length 86 B.
///
/// ```text
/// Offsets begin at the `moCompFace_c` body. The component-path entries follow the marker tail.
/// ```
pub(crate) mod component_face_flagged_operation_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 86;
    /// Offset of `class_token` (`u16`, little-endian). Spec §2.
    pub(crate) const CLASS_TOKEN: usize = 0;
    /// Offset of `record_version` (`u32`, little-endian). Spec §2.
    pub(crate) const RECORD_VERSION: usize = 2;
    /// Offset of `flags` (`bytes[2]`). Spec §2.
    pub(crate) const FLAGS: usize = 6;
    /// Offset of `component_marker` (`bytes[16]`). Spec §2.
    pub(crate) const COMPONENT_MARKER: usize = 68;
    /// Offset of `marker_tail` (`u16`, little-endian). Spec §2.
    pub(crate) const MARKER_TAIL: usize = 84;
}

/// Byte offsets for the `temporary_axis_reference_nine_scalar` record.
///
/// Spec §2. Record length 316 B.
///
/// ```text
/// Offsets begin at the class declaration. The carrier body ends at +311; a following class marker at +312 terminates the record after zero padding of at most 24 bytes.
/// ```
pub(crate) mod temporary_axis_reference_nine_scalar {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 316;
    /// Offset of `class_marker` (`bytes[4]`). Spec §2.
    pub(crate) const CLASS_MARKER: usize = 0;
    /// Stated value of `class_marker` (`bytes[4]`). Spec §2.
    pub(crate) const CLASS_MARKER_VALUE: [u8; 4] = [0xff, 0xff, 0x01, 0x00];
    /// Offset of `name_length` (`u16`, little-endian). Spec §2.
    pub(crate) const NAME_LENGTH: usize = 4;
    /// Stated value of `name_length` (`u16`). Spec §2.
    pub(crate) const NAME_LENGTH_VALUE: u16 = 0x000f;
    /// Offset of `name` (`bytes[15]`). Spec §2.
    pub(crate) const NAME: usize = 6;
    /// Stated value of `name` (`bytes[15]`). Spec §2.
    pub(crate) const NAME_VALUE: [u8; 15] = *b"moTempAxisRef_w";
    /// Offset of `handles` (`bytes[8]`). Spec §2.
    pub(crate) const HANDLES: usize = 223;
    /// Stated value of `handles` (`bytes[8]`). Spec §2.
    pub(crate) const HANDLES_VALUE: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    /// Offset of `zero_before_address` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_BEFORE_ADDRESS: usize = 231;
    /// Stated value of `zero_before_address` (`bytes[4]`). Spec §2.
    pub(crate) const ZERO_BEFORE_ADDRESS_VALUE: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    /// Offset of `stream_address` (`u32`, little-endian). Spec §2.
    pub(crate) const STREAM_ADDRESS: usize = 235;
    /// Offset of `axis_frame` (`f64[9]`, little-endian). Spec §2.
    pub(crate) const AXIS_FRAME: usize = 239;
    /// Offset of `next_class_marker` (`bytes[4]`). Spec §2.
    pub(crate) const NEXT_CLASS_MARKER: usize = 312;
    /// Stated value of `next_class_marker` (`bytes[4]`). Spec §2.
    pub(crate) const NEXT_CLASS_MARKER_VALUE: [u8; 4] = [0xff, 0xff, 0x01, 0x00];
}

/// Byte offsets for the `cosmetic_thread_component_edge_wrapper_prefix` record.
///
/// Spec §2. Record length 17 B.
///
/// ```text
/// Offsets begin at the component-edge body. The compact edge-selection vector or the immediate edge-reference child follows this fixed wrapper prefix.
/// ```
pub(crate) mod cosmetic_thread_component_edge_wrapper_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 17;
    /// Offset of `inner_class_token` (`u16`, little-endian). Spec §2.
    pub(crate) const INNER_CLASS_TOKEN: usize = 0;
    /// Offset of `wrapper_flags` (`bytes[7]`). Spec §2.
    pub(crate) const WRAPPER_FLAGS: usize = 2;
    /// Stated value of `wrapper_flags` (`bytes[7]`). Spec §2.
    pub(crate) const WRAPPER_FLAGS_VALUE: [u8; 7] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    /// Offset of `component_count` (`u32`, little-endian). Spec §2.
    pub(crate) const COMPONENT_COUNT: usize = 9;
    /// Offset of `component_count_copy` (`u32`, little-endian). Spec §2.
    pub(crate) const COMPONENT_COUNT_COPY: usize = 13;
}

/// Byte offsets for the `cosmetic_thread_repeated_edge_ref_prefix` record.
///
/// Spec §2. Record length 8 B.
///
/// ```text
/// Offsets begin at the body opened by the repeated edge-reference class token.
/// ```
pub(crate) mod cosmetic_thread_repeated_edge_ref_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 8;
    /// Offset of `prefix` (`bytes[8]`). Spec §2.
    pub(crate) const PREFIX: usize = 0;
    /// Stated value of `prefix` (`bytes[8]`). Spec §2.
    pub(crate) const PREFIX_VALUE: [u8; 8] = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
}

/// Byte offsets for the `display_lists_scene_source_binding` record.
///
/// Spec §8. Record length 16 B.
pub(crate) mod display_lists_scene_source_binding {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 16;
    /// Offset of `marker` (`bytes[12]`). Spec §8.
    pub(crate) const MARKER: usize = 0;
    /// Stated value of `marker` (`bytes[12]`). Spec §8.
    pub(crate) const MARKER_VALUE: [u8; 12] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0x40, 0x00, 0x00, 0x00, 0x00,
    ];
    /// Offset of `source_id` (`u32`, little-endian). Spec §8.
    pub(crate) const SOURCE_ID: usize = 12;
}

/// Byte offsets for the `display_lists_inline_visual_properties_prefix` record.
///
/// Spec §8. Record length 22 B.
///
/// ```text
/// The variable-length UTF-16LE material name begins at the end of this prefix.
/// ```
pub(crate) mod display_lists_inline_visual_properties_prefix {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 22;
    /// Offset of `marker` (`bytes[2]`). Spec §8.
    pub(crate) const MARKER: usize = 0;
    /// Stated value of `marker` (`bytes[2]`). Spec §8.
    pub(crate) const MARKER_VALUE: [u8; 2] = [0x33, 0x80];
    /// Offset of `packed_color` (`u32`, little-endian). Spec §8.
    pub(crate) const PACKED_COLOR: usize = 2;
    /// Offset of `uninterpreted` (`bytes[12]`). Spec §8.
    pub(crate) const UNINTERPRETED: usize = 6;
    /// Offset of `name_marker` (`bytes[3]`). Spec §8.
    pub(crate) const NAME_MARKER: usize = 18;
    /// Stated value of `name_marker` (`bytes[3]`). Spec §8.
    pub(crate) const NAME_MARKER_VALUE: [u8; 3] = [0xff, 0xfe, 0xff];
    /// Offset of `name_length` (`u8`). Spec §8.
    pub(crate) const NAME_LENGTH: usize = 21;
}

/// Byte offsets for the `visual_states_feature_appearance_prefix` record.
///
/// Spec §8. Record length 36 B.
///
/// ```text
/// The prefix ends after the packed colour. The remaining visual-property payload is outside this fixed layout.
/// ```
pub(crate) mod visual_states_feature_appearance_prefix {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 36;
    /// Offset of `version` (`u32`, little-endian). Spec §8.
    pub(crate) const VERSION: usize = 0;
    /// Stated value of `version` (`u32`). Spec §8.
    pub(crate) const VERSION_VALUE: u32 = 0x0000_4268;
    /// Offset of `feature_source_id` (`u32`, little-endian). Spec §8.
    pub(crate) const FEATURE_SOURCE_ID: usize = 4;
    /// Offset of `feature_timestamp` (`u32`, little-endian). Spec §8.
    pub(crate) const FEATURE_TIMESTAMP: usize = 8;
    /// Offset of `selector_one_a` (`u32`, little-endian). Spec §8.
    pub(crate) const SELECTOR_ONE_A: usize = 12;
    /// Stated value of `selector_one_a` (`u32`). Spec §8.
    pub(crate) const SELECTOR_ONE_A_VALUE: u32 = 0x0000_0001;
    /// Offset of `selector_one_b` (`u32`, little-endian). Spec §8.
    pub(crate) const SELECTOR_ONE_B: usize = 16;
    /// Stated value of `selector_one_b` (`u32`). Spec §8.
    pub(crate) const SELECTOR_ONE_B_VALUE: u32 = 0x0000_0001;
    /// Offset of `selector_two` (`u32`, little-endian). Spec §8.
    pub(crate) const SELECTOR_TWO: usize = 20;
    /// Stated value of `selector_two` (`u32`). Spec §8.
    pub(crate) const SELECTOR_TWO_VALUE: u32 = 0x0000_0002;
    /// Offset of `instance_prefix` (`bytes[6]`). Spec §8.
    pub(crate) const INSTANCE_PREFIX: usize = 24;
    /// Stated value of `instance_prefix` (`bytes[6]`). Spec §8.
    pub(crate) const INSTANCE_PREFIX_VALUE: [u8; 6] = [0x07, 0x80, 0x01, 0x00, 0x00, 0x00];
    /// Offset of `marker` (`bytes[2]`). Spec §8.
    pub(crate) const MARKER: usize = 30;
    /// Stated value of `marker` (`bytes[2]`). Spec §8.
    pub(crate) const MARKER_VALUE: [u8; 2] = [0x09, 0x80];
    /// Offset of `packed_color` (`u32`, little-endian). Spec §8.
    pub(crate) const PACKED_COLOR: usize = 32;
}

/// Byte offsets for the `transformed_reference_plane_metadata` record.
///
/// Spec §8. Record length 80 B.
///
/// ```text
/// Offsets begin immediately after the `moTransRefPlaneData_c` class token.
/// ```
pub(crate) mod transformed_reference_plane_metadata {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 80;
    /// Offset of `prefix` (`bytes[8]`). Spec §8.
    pub(crate) const PREFIX: usize = 0;
    /// Stated value of `prefix` (`bytes[8]`). Spec §8.
    pub(crate) const PREFIX_VALUE: [u8; 8] = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    /// Offset of `center` (`f64[3]`, little-endian). Spec §8.
    pub(crate) const CENTER: usize = 8;
    /// Offset of `extents` (`f64[2]`, little-endian). Spec §8.
    pub(crate) const EXTENTS: usize = 32;
    /// Offset of `auxiliary_frame` (`f64[3]`, little-endian). Spec §8.
    pub(crate) const AUXILIARY_FRAME: usize = 48;
    /// Offset of `diagonal` (`f64`, little-endian). Spec §8.
    pub(crate) const DIAGONAL: usize = 72;
}

/// Byte offsets for the `display_lists_compact_face_header` record.
///
/// Spec §8. Record length 8 B.
///
/// ```text
/// Offsets begin after the `uoTempFaceTessData_c` class token. The first descriptor starts at the end of this compact header.
/// ```
pub(crate) mod display_lists_compact_face_header {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 8;
    /// Offset of `triangle_count` (`u32`, little-endian). Spec §8.
    pub(crate) const TRIANGLE_COUNT: usize = 0;
    /// Offset of `strip_count` (`u32`, little-endian). Spec §8.
    pub(crate) const STRIP_COUNT: usize = 4;
}

/// Byte offsets for the `display_lists_extended_face_header` record.
///
/// Spec §8. Record length 40 B.
///
/// ```text
/// Offsets begin after the `uoTempFaceTessData_c` class token. The first descriptor starts at the end of this extended header.
/// ```
pub(crate) mod display_lists_extended_face_header {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 40;
    /// Offset of `triangle_count` (`u32`, little-endian). Spec §8.
    pub(crate) const TRIANGLE_COUNT: usize = 0;
    /// Offset of `strip_count` (`u32`, little-endian). Spec §8.
    pub(crate) const STRIP_COUNT: usize = 4;
    /// Offset of `form` (`u32`, little-endian). Spec §8.
    pub(crate) const FORM: usize = 8;
    /// Offset of `zero_at_12` (`u32`, little-endian). Spec §8.
    pub(crate) const ZERO_AT_12: usize = 12;
    /// Offset of `zero_at_16` (`u32`, little-endian). Spec §8.
    pub(crate) const ZERO_AT_16: usize = 16;
    /// Offset of `form_token` (`u32`, little-endian). Spec §8.
    pub(crate) const FORM_TOKEN: usize = 20;
    /// Offset of `zero_tail` (`bytes[16]`). Spec §8.
    pub(crate) const ZERO_TAIL: usize = 24;
}

/// Byte offsets for the `draft_plane_reference_prefix` record.
///
/// Spec §2. Record length 112 B.
///
/// ```text
/// The variable component-path entries follow this prefix. Offsets begin at the lane-scoped plane-reference token.
/// ```
pub(crate) mod draft_plane_reference_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 112;
    /// Offset of `token` (`u16`, little-endian). Spec §2.
    pub(crate) const TOKEN: usize = 0;
    /// Offset of `child_token` (`u16`, little-endian). Spec §2.
    pub(crate) const CHILD_TOKEN: usize = 2;
    /// Offset of `form` (`u32`, little-endian). Spec §2.
    pub(crate) const FORM: usize = 4;
    /// Offset of `wrapper_flags` (`bytes[3]`). Spec §2.
    pub(crate) const WRAPPER_FLAGS: usize = 8;
    /// Offset of `identity` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY: usize = 11;
    /// Offset of `identity_copy` (`u32`, little-endian). Spec §2.
    pub(crate) const IDENTITY_COPY: usize = 15;
    /// Offset of `sentinel` (`bytes[16]`). Spec §2.
    pub(crate) const SENTINEL: usize = 47;
    /// Offset of `instance_token` (`u16`, little-endian). Spec §2.
    pub(crate) const INSTANCE_TOKEN: usize = 72;
    /// Offset of `role` (`u32`, little-endian). Spec §2.
    pub(crate) const ROLE: usize = 74;
    /// Offset of `zero_at_78` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_AT_78: usize = 78;
    /// Offset of `cell_count` (`u32`, little-endian). Spec §2.
    pub(crate) const CELL_COUNT: usize = 82;
    /// Offset of `path_kind` (`bytes[4]`). Spec §2.
    pub(crate) const PATH_KIND: usize = 86;
    /// Offset of `selector` (`u32`, little-endian). Spec §2.
    pub(crate) const SELECTOR: usize = 90;
    /// Offset of `component_marker` (`bytes[16]`). Spec §2.
    pub(crate) const COMPONENT_MARKER: usize = 94;
    /// Offset of `marker_tail` (`u16`, little-endian). Spec §2.
    pub(crate) const MARKER_TAIL: usize = 110;
}

/// Byte offsets for the `draft_compact_selection_prefix` record.
///
/// Spec §2. Record length 30 B.
///
/// ```text
/// Variable mixed component paths follow this prefix. Offsets begin at the bounded cell field.
/// ```
pub(crate) mod draft_compact_selection_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 30;
    /// Offset of `cell_field` (`u32`, little-endian). Spec §2.
    pub(crate) const CELL_FIELD: usize = 0;
    /// Offset of `selection_role` (`bytes[4]`). Spec §2.
    pub(crate) const SELECTION_ROLE: usize = 4;
    /// Offset of `selector` (`u32`, little-endian). Spec §2.
    pub(crate) const SELECTOR: usize = 8;
    /// Offset of `component_marker` (`bytes[16]`). Spec §2.
    pub(crate) const COMPONENT_MARKER: usize = 12;
    /// Offset of `marker_tail` (`u16`, little-endian). Spec §2.
    pub(crate) const MARKER_TAIL: usize = 28;
}

/// Byte offsets for the `draft_aligned_direction_frame` record.
///
/// Spec §2. Record length 120 B.
pub(crate) mod draft_aligned_direction_frame {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 120;
    /// Offset of `handles` (`bytes[8]`). Spec §2.
    pub(crate) const HANDLES: usize = 0;
    /// Offset of `zero_at_8` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_AT_8: usize = 8;
    /// Offset of `address` (`u32`, little-endian). Spec §2.
    pub(crate) const ADDRESS: usize = 12;
    /// Offset of `pull_direction` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const PULL_DIRECTION: usize = 96;
}

/// Byte offsets for the `draft_extended_direction_frame` record.
///
/// Spec §2. Record length 153 B.
pub(crate) mod draft_extended_direction_frame {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 153;
    /// Offset of `handles` (`bytes[8]`). Spec §2.
    pub(crate) const HANDLES: usize = 0;
    /// Offset of `zero_at_8` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_AT_8: usize = 8;
    /// Offset of `address` (`u32`, little-endian). Spec §2.
    pub(crate) const ADDRESS: usize = 12;
    /// Offset of `pull_direction` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const PULL_DIRECTION: usize = 129;
}

/// Byte offsets for the `current_indexed_spatial_xyz_point_prefix` record.
///
/// Spec §2. Record length 96 B.
///
/// ```text
/// The fixed prefix ends at the relation terminator. The following native tail is bounded by a sketch marker at +158 or +162 after a four-byte separator, or by the terminal reference-table prefix.
/// ```
pub(crate) mod current_indexed_spatial_xyz_point_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 96;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `profile_role` (`u16`, little-endian). Spec §2.
    pub(crate) const PROFILE_ROLE: usize = 27;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinates` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const COORDINATES: usize = 58;
    /// Offset of `tail_word_0` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_WORD_0: usize = 82;
    /// Offset of `tail_word_1` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_WORD_1: usize = 84;
    /// Offset of `tail_zero` (`bytes[6]`). Spec §2.
    pub(crate) const TAIL_ZERO: usize = 86;
    /// Offset of `terminator` (`bytes[4]`). Spec §2.
    pub(crate) const TERMINATOR: usize = 92;
}

/// Byte offsets for the `current_indexed_spatial_xyz_terminal_reference_prefix_short` record.
///
/// Spec §2. Record length 330 B.
///
/// ```text
/// The terminal prefix starts after the relation terminator. Its ten-byte alignment suffix places the table header at marker +232; the variable reference-table body follows the fixed control sequence.
/// ```
pub(crate) mod current_indexed_spatial_xyz_terminal_reference_prefix_short {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 330;
    /// Offset of `tail_word_0` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_WORD_0: usize = 82;
    /// Offset of `tail_word_1` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_WORD_1: usize = 84;
    /// Offset of `terminator` (`bytes[4]`). Spec §2.
    pub(crate) const TERMINATOR: usize = 92;
    /// Offset of `zero_after_terminator` (`bytes[124]`). Spec §2.
    pub(crate) const ZERO_AFTER_TERMINATOR: usize = 96;
    /// Offset of `terminal_tag` (`bytes[2]`). Spec §2.
    pub(crate) const TERMINAL_TAG: usize = 220;
    /// Offset of `zero_alignment_suffix` (`bytes[10]`). Spec §2.
    pub(crate) const ZERO_ALIGNMENT_SUFFIX: usize = 222;
    /// Offset of `table_header` (`bytes[4]`). Spec §2.
    pub(crate) const TABLE_HEADER: usize = 232;
    /// Offset of `first_count` (`u32`, little-endian). Spec §2.
    pub(crate) const FIRST_COUNT: usize = 236;
    /// Offset of `second_count` (`u32`, little-endian). Spec §2.
    pub(crate) const SECOND_COUNT: usize = 240;
    /// Offset of `one_run` (`bytes[48]`). Spec §2.
    pub(crate) const ONE_RUN: usize = 244;
    /// Offset of `zero_after_one_run` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_AFTER_ONE_RUN: usize = 292;
    /// Offset of `one_after_zero` (`u32`, little-endian). Spec §2.
    pub(crate) const ONE_AFTER_ZERO: usize = 296;
    /// Offset of `zero_before_control` (`bytes[6]`). Spec §2.
    pub(crate) const ZERO_BEFORE_CONTROL: usize = 300;
    /// Offset of `control_sequence` (`bytes[24]`). Spec §2.
    pub(crate) const CONTROL_SEQUENCE: usize = 306;
}

/// Byte offsets for the `current_indexed_spatial_xyz_terminal_reference_prefix_long` record.
///
/// Spec §2. Record length 334 B.
///
/// ```text
/// The terminal prefix starts after the relation terminator. Its fourteen-byte alignment suffix places the table header at marker +236; the variable reference-table body follows the fixed control sequence.
/// ```
pub(crate) mod current_indexed_spatial_xyz_terminal_reference_prefix_long {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 334;
    /// Offset of `tail_word_0` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_WORD_0: usize = 82;
    /// Offset of `tail_word_1` (`u16`, little-endian). Spec §2.
    pub(crate) const TAIL_WORD_1: usize = 84;
    /// Offset of `terminator` (`bytes[4]`). Spec §2.
    pub(crate) const TERMINATOR: usize = 92;
    /// Offset of `zero_after_terminator` (`bytes[124]`). Spec §2.
    pub(crate) const ZERO_AFTER_TERMINATOR: usize = 96;
    /// Offset of `terminal_tag` (`bytes[2]`). Spec §2.
    pub(crate) const TERMINAL_TAG: usize = 220;
    /// Offset of `zero_alignment_suffix` (`bytes[14]`). Spec §2.
    pub(crate) const ZERO_ALIGNMENT_SUFFIX: usize = 222;
    /// Offset of `table_header` (`bytes[4]`). Spec §2.
    pub(crate) const TABLE_HEADER: usize = 236;
    /// Offset of `first_count` (`u32`, little-endian). Spec §2.
    pub(crate) const FIRST_COUNT: usize = 240;
    /// Offset of `second_count` (`u32`, little-endian). Spec §2.
    pub(crate) const SECOND_COUNT: usize = 244;
    /// Offset of `one_run` (`bytes[48]`). Spec §2.
    pub(crate) const ONE_RUN: usize = 248;
    /// Offset of `zero_after_one_run` (`u32`, little-endian). Spec §2.
    pub(crate) const ZERO_AFTER_ONE_RUN: usize = 296;
    /// Offset of `one_after_zero` (`u32`, little-endian). Spec §2.
    pub(crate) const ONE_AFTER_ZERO: usize = 300;
    /// Offset of `zero_before_control` (`bytes[6]`). Spec §2.
    pub(crate) const ZERO_BEFORE_CONTROL: usize = 304;
    /// Offset of `control_sequence` (`bytes[24]`). Spec §2.
    pub(crate) const CONTROL_SEQUENCE: usize = 310;
}

/// Byte offsets for the `compact_current_spatial_marker_point` record.
///
/// Spec §2. Record length 82 B.
///
/// ```text
/// The compact point prefix ends after the third coordinate. The compact form is complete only at a next sketch marker at +82, at a next marker after a four-byte separator at +86, or at the feature-input lane end.
/// ```
pub(crate) mod compact_current_spatial_marker_point {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 82;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `profile_role` (`u16`, little-endian). Spec §2.
    pub(crate) const PROFILE_ROLE: usize = 27;
    /// Offset of `selector` (`bytes[8]`). Spec §2.
    pub(crate) const SELECTOR: usize = 31;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 56;
    /// Offset of `coordinates` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const COORDINATES: usize = 58;
}

/// Byte offsets for the `wide_spatial_marker_coordinate_prefix` record.
///
/// Spec §2. Record length 90 B.
///
/// ```text
/// The fixed coordinate prefix is shared by point and relation-handle markers. The record trailer follows the third coordinate.
/// ```
pub(crate) mod wide_spatial_marker_coordinate_prefix {
    /// Record length in bytes. Spec §2.
    pub(crate) const LEN: usize = 90;
    /// Offset of `marker` (`bytes[5]`). Spec §2.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `header` (`bytes[8]`). Spec §2.
    pub(crate) const HEADER: usize = 5;
    /// Offset of `sentinel` (`f32`, little-endian). Spec §2.
    pub(crate) const SENTINEL: usize = 13;
    /// Offset of `native_kind` (`u32`, little-endian). Spec §2.
    pub(crate) const NATIVE_KIND: usize = 17;
    /// Offset of `profile_locus` (`bytes[4]`). Spec §2.
    pub(crate) const PROFILE_LOCUS: usize = 23;
    /// Offset of `profile_role` (`u16`, little-endian). Spec §2.
    pub(crate) const PROFILE_ROLE: usize = 27;
    /// Offset of `state_value` (`f64`, little-endian). Spec §2.
    pub(crate) const STATE_VALUE: usize = 48;
    /// Offset of `coordinate_tag` (`bytes[2]`). Spec §2.
    pub(crate) const COORDINATE_TAG: usize = 64;
    /// Offset of `coordinates` (`f64[3]`, little-endian). Spec §2.
    pub(crate) const COORDINATES: usize = 66;
}
