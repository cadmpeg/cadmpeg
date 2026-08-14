// SPDX-License-Identifier: Apache-2.0
//! Byte-offset and value constants generated from `docs/layouts/catia.toml`.
//!
//! Do not edit by hand. Regenerate with:
//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`.

#![allow(dead_code)] // Not every generated constant is referenced yet.

// Records omitted because the table declares a contradiction.
//
// - `e5_record_frame` (size_mismatch): The enumerated header fields total 14 bytes but the same sentence states the record stride as `payload_size + 13`, which implies a 13-byte header. The parser follows both at once: it advances by `size + 13` yet decodes carrier fields from record `+14`, and checks the `0xff` edge-use lead byte at record `+13`. The spec does not say which of the two numbers is authoritative.

/// Tag constants from the table inventory.
pub(crate) mod token {
    /// `named stream block` (`FINJPL  `). Spec §4.
    pub(crate) const NAMED_STREAM_BLOCK: [u8; 8] = *b"FINJPL  ";
    /// `source-schema string catalog` (`7C 02`). Spec §4.
    pub(crate) const SOURCE_SCHEMA_STRING_CATALOG: [u8; 2] = [0x7c, 0x02];
    /// `literal float data` (`7C D9`). Spec §4.
    pub(crate) const LITERAL_FLOAT_DATA: [u8; 2] = [0x7c, 0xd9];
    /// `standard edge-table delimiter` (`10 24 04 ff ff 00 00 00`). Spec §4.
    pub(crate) const STANDARD_EDGE_TABLE_DELIMITER: [u8; 8] = [0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
    /// `vertex XYZ record` (`05 08 01`). Spec §4.
    pub(crate) const VERTEX_XYZ_RECORD: [u8; 3] = [0x05, 0x08, 0x01];
    /// `zero-entity record family` (`a9 03`). Spec §4.
    pub(crate) const ZERO_ENTITY_RECORD_FAMILY: [u8; 2] = [0xa9, 0x03];
    /// `E5 record family` (`E5 0D 03`). Spec §4.
    pub(crate) const E5_RECORD_FAMILY: [u8; 3] = [0xe5, 0x0d, 0x03];
}

/// Byte offsets for the `outer_header` record.
///
/// Spec §3.1. Record length 64 B.
///
/// ```text
/// `directory_offset + directory_length == file_size`. The parser reads only the magic and the two directory words; the fill and flag regions are never read.
/// ```
pub(crate) mod outer_header {
    /// Record length in bytes. Spec §3.1.
    pub(crate) const LEN: usize = 64;
    /// Offset of `magic` (`bytes[8]`). Spec §3.1.
    pub(crate) const MAGIC: usize = 0;
    /// Stated value of `magic` (`bytes[8]`). Spec §3.1.
    pub(crate) const MAGIC_VALUE: [u8; 8] = *b"V5_CFV2\0";
    /// Offset of `directory_offset` (`u32`, big-endian). Spec §3.1.
    pub(crate) const DIRECTORY_OFFSET: usize = 8;
    /// Offset of `directory_length` (`u32`, big-endian). Spec §3.1.
    pub(crate) const DIRECTORY_LENGTH: usize = 12;
    /// Offset of `fill_ff` (`bytes[8]`). Spec §3.1.
    pub(crate) const FILL_FF: usize = 16;
    /// Offset of `fill_00` (`bytes[32]`). Spec §3.1.
    pub(crate) const FILL_00: usize = 24;
    /// Offset of `hdr_flags` (`bytes[8]`). Spec §3.1.
    pub(crate) const HDR_FLAGS: usize = 56;
}

/// Byte offsets for the `inner_header` record.
///
/// Spec §3.2. Record length 16 B.
///
/// ```text
/// `inner` is the first `V5_CFV2\0` after outer byte 8. `diroff = inner + A`.
/// ```
pub(crate) mod inner_header {
    /// Record length in bytes. Spec §3.2.
    pub(crate) const LEN: usize = 16;
    /// Offset of `magic` (`bytes[8]`). Spec §3.2.
    pub(crate) const MAGIC: usize = 0;
    /// Offset of `directory_offset_delta` (`u32`, big-endian). Spec §3.2.
    pub(crate) const DIRECTORY_OFFSET_DELTA: usize = 8;
    /// Offset of `directory_length` (`u32`, big-endian). Spec §3.2.
    pub(crate) const DIRECTORY_LENGTH: usize = 12;
}

/// Byte offsets for the `stream_descriptor_header` record.
///
/// Spec §3.4. Record length 84 B.
///
/// ```text
/// Descriptor-relative. `k` extent structs of 20 bytes each follow at ds+0x54. The standard name form ends at the three-byte tail ds-3..ds (`00 00 00`); the legacy form starts at ds+0x10 and ends with the same UTF-16LE terminator, with zero fill through ds+0x50.
/// ```
pub(crate) mod stream_descriptor_header {
    /// Record length in bytes. Spec §3.4.
    pub(crate) const LEN: usize = 84;
    /// Offset of `logical_stream_length` (`u32`, big-endian). Spec §3.4.
    pub(crate) const LOGICAL_STREAM_LENGTH: usize = 12;
    /// Offset of `extent_count` (`u32`, big-endian). Spec §3.4.
    pub(crate) const EXTENT_COUNT: usize = 80;
}

/// Byte offsets for the `extent_struct` record.
///
/// Spec §3.4. Record length 20 B.
///
/// ```text
/// `inner + phys_off + phys_len <= filesize`, `phys_len != 0`, `log_off` cumulative from 0, `log_len == phys_len`, and `sum(log_len) == logical_stream_length`.
/// ```
pub(crate) mod extent_struct {
    /// Record length in bytes. Spec §3.4.
    pub(crate) const LEN: usize = 20;
    /// Offset of `phys_off` (`u32`, big-endian). Spec §3.4.
    pub(crate) const PHYS_OFF: usize = 0;
    /// Offset of `phys_len` (`u32`, big-endian). Spec §3.4.
    pub(crate) const PHYS_LEN: usize = 4;
    /// Offset of `log_len` (`u32`, big-endian). Spec §3.4.
    pub(crate) const LOG_LEN: usize = 8;
    /// Offset of `log_off` (`u32`, big-endian). Spec §3.4.
    pub(crate) const LOG_OFF: usize = 12;
    /// Offset of `flags` (`u32`, big-endian). Spec §3.4.
    pub(crate) const FLAGS: usize = 16;
}

/// Byte offsets for the `vertex_roster_row` record.
///
/// Spec §3.5. Record length 7 B.
///
/// ```text
/// The tags are unique and strictly increasing across the run.
/// ```
pub(crate) mod vertex_roster_row {
    /// Record length in bytes. Spec §3.5.
    pub(crate) const LEN: usize = 7;
    /// Offset of `marker` (`u8`). Spec §3.5.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `tag` (`u24`, little-endian). Spec §3.5.
    pub(crate) const TAG: usize = 1;
    /// Offset of `zero_run` (`bytes[3]`). Spec §3.5.
    pub(crate) const ZERO_RUN: usize = 4;
}

/// Byte offsets for the `freeform_surface_core` record.
///
/// Spec §3.5. Record length 47 B.
///
/// ```text
/// `f[0:3]` is the trimmed face's AABB centre, `f[3:6]` its AABB half-extents, `f[6:9]` its bounding-sphere centre, and `f[9]` the bounding-sphere radius. The containment invariant `|f[i]−f[6+i]| + f[3+i] ≤ f[9]` holds.
/// ```
pub(crate) mod freeform_surface_core {
    /// Record length in bytes. Spec §3.5.
    pub(crate) const LEN: usize = 47;
    /// Offset of `tag` (`u24`, little-endian). Spec §3.5.
    pub(crate) const TAG: usize = 0;
    /// Offset of `zero_run` (`bytes[3]`). Spec §3.5.
    pub(crate) const ZERO_RUN: usize = 3;
    /// Offset of `bounds` (`f32[10]`, little-endian). Spec §3.5.
    pub(crate) const BOUNDS: usize = 6;
    /// Offset of `sign` (`i8`). Spec §3.5.
    pub(crate) const SIGN: usize = 46;
}

/// Byte offsets for the `analytic_surface_plane` record.
///
/// Spec §5.8. Record length 49 B.
///
/// ```text
/// Record start is `marker_pos − 5`. Grammar: `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>`. The payload holds BE f32 parameters; the spec states its slot order but no per-slot offsets.
/// ```
pub(crate) mod analytic_surface_plane {
    /// Record length in bytes. Spec §5.8.
    pub(crate) const LEN: usize = 49;
    /// Offset of `target_tag` (`u24`, little-endian). Spec §5.8.
    pub(crate) const TARGET_TAG: usize = 0;
    /// Offset of `zero` (`u8`). Spec §5.8.
    pub(crate) const ZERO: usize = 3;
    /// Offset of `prebyte` (`u8`). Spec §5.8.
    pub(crate) const PREBYTE: usize = 4;
    /// Offset of `marker` (`bytes[2]`). Spec §5.8.
    pub(crate) const MARKER: usize = 5;
    /// Offset of `kind` (`u8`). Spec §5.8.
    pub(crate) const KIND: usize = 7;
    /// Offset of `sign` (`i8`). Spec §5.8.
    pub(crate) const SIGN: usize = 48;
}

/// Byte offsets for the `analytic_surface_cylinder` record.
///
/// Spec §5.8. Record length 73 B.
///
/// ```text
/// Cylinder and cone share prebyte and length; the kind byte distinguishes them. Payload slots are `[px py pz ax ay radius]` as BE f32.
/// ```
pub(crate) mod analytic_surface_cylinder {
    /// Record length in bytes. Spec §5.8.
    pub(crate) const LEN: usize = 73;
    /// Offset of `target_tag` (`u24`, little-endian). Spec §5.8.
    pub(crate) const TARGET_TAG: usize = 0;
    /// Offset of `zero` (`u8`). Spec §5.8.
    pub(crate) const ZERO: usize = 3;
    /// Offset of `prebyte` (`u8`). Spec §5.8.
    pub(crate) const PREBYTE: usize = 4;
    /// Offset of `marker` (`bytes[2]`). Spec §5.8.
    pub(crate) const MARKER: usize = 5;
    /// Offset of `kind` (`u8`). Spec §5.8.
    pub(crate) const KIND: usize = 7;
    /// Offset of `sign` (`i8`). Spec §5.8.
    pub(crate) const SIGN: usize = 72;
}

/// Byte offsets for the `analytic_surface_cone` record.
///
/// Spec §5.8. Record length 73 B.
pub(crate) mod analytic_surface_cone {
    /// Record length in bytes. Spec §5.8.
    pub(crate) const LEN: usize = 73;
    /// Offset of `target_tag` (`u24`, little-endian). Spec §5.8.
    pub(crate) const TARGET_TAG: usize = 0;
    /// Offset of `zero` (`u8`). Spec §5.8.
    pub(crate) const ZERO: usize = 3;
    /// Offset of `prebyte` (`u8`). Spec §5.8.
    pub(crate) const PREBYTE: usize = 4;
    /// Offset of `marker` (`bytes[2]`). Spec §5.8.
    pub(crate) const MARKER: usize = 5;
    /// Offset of `kind` (`u8`). Spec §5.8.
    pub(crate) const KIND: usize = 7;
    /// Offset of `sign` (`i8`). Spec §5.8.
    pub(crate) const SIGN: usize = 72;
}

/// Byte offsets for the `analytic_surface_sphere` record.
///
/// Spec §5.8. Record length 65 B.
pub(crate) mod analytic_surface_sphere {
    /// Record length in bytes. Spec §5.8.
    pub(crate) const LEN: usize = 65;
    /// Offset of `target_tag` (`u24`, little-endian). Spec §5.8.
    pub(crate) const TARGET_TAG: usize = 0;
    /// Offset of `zero` (`u8`). Spec §5.8.
    pub(crate) const ZERO: usize = 3;
    /// Offset of `prebyte` (`u8`). Spec §5.8.
    pub(crate) const PREBYTE: usize = 4;
    /// Offset of `marker` (`bytes[2]`). Spec §5.8.
    pub(crate) const MARKER: usize = 5;
    /// Offset of `kind` (`u8`). Spec §5.8.
    pub(crate) const KIND: usize = 7;
    /// Offset of `sign` (`i8`). Spec §5.8.
    pub(crate) const SIGN: usize = 64;
}

/// Byte offsets for the `analytic_surface_torus` record.
///
/// Spec §5.8. Record length 77 B.
pub(crate) mod analytic_surface_torus {
    /// Record length in bytes. Spec §5.8.
    pub(crate) const LEN: usize = 77;
    /// Offset of `target_tag` (`u24`, little-endian). Spec §5.8.
    pub(crate) const TARGET_TAG: usize = 0;
    /// Offset of `zero` (`u8`). Spec §5.8.
    pub(crate) const ZERO: usize = 3;
    /// Offset of `prebyte` (`u8`). Spec §5.8.
    pub(crate) const PREBYTE: usize = 4;
    /// Offset of `marker` (`bytes[2]`). Spec §5.8.
    pub(crate) const MARKER: usize = 5;
    /// Offset of `kind` (`u8`). Spec §5.8.
    pub(crate) const KIND: usize = 7;
    /// Offset of `sign` (`i8`). Spec §5.8.
    pub(crate) const SIGN: usize = 76;
}

/// Byte offsets for the `a_family_frame` record.
///
/// Spec §6. Record length 7 B.
///
/// ```text
/// Header only; the width-`W` header token occupies +7..+7+W and the payload starts at +7+W. `next = +7+W+payload_len`. The header token is a small repeating type code, not a per-record object id.
/// ```
pub(crate) mod a_family_frame {
    /// Record length in bytes. Spec §6.
    pub(crate) const LEN: usize = 7;
    /// Offset of `family` (`u8`). Spec §6.
    pub(crate) const FAMILY: usize = 0;
    /// Offset of `flag` (`u8`). Spec §6.
    pub(crate) const FLAG: usize = 1;
    /// Offset of `class` (`u8`). Spec §6.
    pub(crate) const CLASS: usize = 2;
    /// Offset of `payload_len` (`u32`, little-endian). Spec §6.
    pub(crate) const PAYLOAD_LEN: usize = 3;
}

/// Byte offsets for the `b_family_frame` record.
///
/// Spec §6. Record length 4 B.
///
/// ```text
/// Header only; the width-`W` header token occupies +4..+4+W and the payload starts at +4+W. `next = +4+W+payload_len`.
/// ```
pub(crate) mod b_family_frame {
    /// Record length in bytes. Spec §6.
    pub(crate) const LEN: usize = 4;
    /// Offset of `family` (`u8`). Spec §6.
    pub(crate) const FAMILY: usize = 0;
    /// Offset of `flag` (`u8`). Spec §6.
    pub(crate) const FLAG: usize = 1;
    /// Offset of `class` (`u8`). Spec §6.
    pub(crate) const CLASS: usize = 2;
    /// Offset of `payload_len` (`u8`). Spec §6.
    pub(crate) const PAYLOAD_LEN: usize = 3;
}

/// Byte offsets for the `a8_object_stream_frame` record.
///
/// Spec §6.6. Record length 11 B.
///
/// ```text
/// `frame_flag` is `03`, `13`, or `83`. References inside the payload are compact tokens selecting an id width (`18` selects u16, `38` selects u24).
/// ```
pub(crate) mod a8_object_stream_frame {
    /// Record length in bytes. Spec §6.6.
    pub(crate) const LEN: usize = 11;
    /// Offset of `family` (`u8`). Spec §6.6.
    pub(crate) const FAMILY: usize = 0;
    /// Offset of `frame_flag` (`u8`). Spec §6.6.
    pub(crate) const FRAME_FLAG: usize = 1;
    /// Offset of `class` (`u8`). Spec §6.6.
    pub(crate) const CLASS: usize = 2;
    /// Offset of `payload_len` (`u32`, little-endian). Spec §6.6.
    pub(crate) const PAYLOAD_LEN: usize = 3;
    /// Offset of `object_id` (`u32`, little-endian). Spec §6.6.
    pub(crate) const OBJECT_ID: usize = 7;
}

/// Byte offsets for the `surface_of_revolution_b2_03_2d` record.
///
/// Spec §5.15. Record length 174 B.
///
/// ```text
/// Three normalized relations hold to f64 bit-equality: `angular_lo/scale==0.5`, `(angular_hi−angular_lo)/scale==2π`, and `mean/scale==π+0.5`.
/// ```
pub(crate) mod surface_of_revolution_b2_03_2d {
    /// Record length in bytes. Spec §5.15.
    pub(crate) const LEN: usize = 174;
    /// Offset of `reference_token` (`u8`). Spec §5.15.
    pub(crate) const REFERENCE_TOKEN: usize = 5;
    /// Offset of `profile_allocation_identity` (`u16`, little-endian). Spec §5.15.
    pub(crate) const PROFILE_ALLOCATION_IDENTITY: usize = 6;
    /// Offset of `frame` (`f64[12]`, little-endian). Spec §5.15.
    pub(crate) const FRAME: usize = 8;
    /// Offset of `bounds` (`f64[4]`, little-endian). Spec §5.15.
    pub(crate) const BOUNDS: usize = 104;
}

/// Byte offsets for the `a9_03_frame` record.
///
/// Spec §8. Record length 4 B.
///
/// ```text
/// Header only; the payload of `YY + 8` bytes follows at +4, so the record length is `YY + 12`. Records reference each other by one-based global record ordinal into the `a9 03` stream.
/// ```
pub(crate) mod a9_03_frame {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 4;
    /// Offset of `family` (`bytes[2]`). Spec §8.
    pub(crate) const FAMILY: usize = 0;
    /// Offset of `tag_hi` (`u8`). Spec §8.
    pub(crate) const TAG_HI: usize = 2;
    /// Offset of `tag_lo_length_driver` (`u8`). Spec §8.
    pub(crate) const TAG_LO_LENGTH_DRIVER: usize = 3;
}

/// Byte offsets for the `zero_entity_edge_stride_5e1a` record.
///
/// Spec §8. Record length 38 B.
///
/// ```text
/// Each tagged allocation value is one tag byte plus a little-endian u32, so the five values run at stride 5.
/// ```
pub(crate) mod zero_entity_edge_stride_5e1a {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 38;
    /// Offset of `tagged_one_prefix` (`bytes[5]`). Spec §8.
    pub(crate) const TAGGED_ONE_PREFIX: usize = 7;
    /// Offset of `allocations` (`bytes[25]`). Spec §8.
    pub(crate) const ALLOCATIONS: usize = 12;
    /// Offset of `terminal` (`u8`). Spec §8.
    pub(crate) const TERMINAL: usize = 37;
}

/// Byte offsets for the `zero_entity_vertex_owner_5d06` record.
///
/// Spec §8. Record length 18 B.
pub(crate) mod zero_entity_vertex_owner_5d06 {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 18;
    /// Offset of `tagged_one_a` (`bytes[5]`). Spec §8.
    pub(crate) const TAGGED_ONE_A: usize = 7;
    /// Offset of `tagged_one_b` (`bytes[5]`). Spec §8.
    pub(crate) const TAGGED_ONE_B: usize = 12;
    /// Offset of `terminal` (`u8`). Spec §8.
    pub(crate) const TERMINAL: usize = 17;
}

/// Byte offsets for the `zero_entity_pcurve_2171` record.
///
/// Spec §8. Record length 125 B.
///
/// ```text
/// One row of the §8 inline support-pcurve family table. Distinct f64 knots are followed by equally many tagged u32 multiplicities; `degree = first_multiplicity - 1` and `control_count = sum(multiplicities) - degree - 1`. The 125-byte total is the parser's required end for this tag; §8 states logical lengths only for `2145`, `2172`, and `219f`.
/// ```
pub(crate) mod zero_entity_pcurve_2171 {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 125;
    /// Offset of `knots` (`f64[2]`, little-endian). Spec §8.
    pub(crate) const KNOTS: usize = 67;
    /// Offset of `multiplicities` (`bytes[10]`). Spec §8.
    pub(crate) const MULTIPLICITIES: usize = 83;
    /// Offset of `poles` (`f64[4]`, little-endian). Spec §8.
    pub(crate) const POLES: usize = 93;
}

/// Byte offsets for the `zero_entity_34c8_pole_grid` record.
///
/// Spec §8. Record length 1176 B.
///
/// ```text
/// This sub-layout starts at the carrier-relative pole-grid offset +167. The variable knot and dimension lanes before it are bounded by this fixed continuation boundary.
/// ```
pub(crate) mod zero_entity_34c8_pole_grid {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 1176;
    /// Offset of `poles` (`f64[147]`, little-endian). Spec §8.
    pub(crate) const POLES: usize = 0;
}

/// Byte offsets for the `zero_entity_345e_pole_grid` record.
///
/// Spec §8. Record length 840 B.
///
/// ```text
/// This sub-layout starts at the carrier-relative pole-grid offset +141. The variable knot and dimension lanes before it are bounded by this fixed continuation boundary.
/// ```
pub(crate) mod zero_entity_345e_pole_grid {
    /// Record length in bytes. Spec §8.
    pub(crate) const LEN: usize = 840;
    /// Offset of `poles` (`f64[105]`, little-endian). Spec §8.
    pub(crate) const POLES: usize = 0;
}

/// Byte offsets for the `value_block_7c0b` record.
///
/// Spec §7.4. Record length 6 B.
///
/// ```text
/// Header only. `declared_len` measures from the `7C0B` marker through the byte before the terminator, so the complete block occupies `declared_len + 1` bytes: payload of `declared_len - 6` bytes at +6, the `FE` terminator at `+declared_len`, then the associated `7C02` catalog.
/// ```
pub(crate) mod value_block_7c0b {
    /// Record length in bytes. Spec §7.4.
    pub(crate) const LEN: usize = 6;
    /// Offset of `marker` (`bytes[2]`). Spec §7.4.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `declared_len` (`u32`, little-endian). Spec §7.4.
    pub(crate) const DECLARED_LEN: usize = 2;
}

/// Byte offsets for the `outer_alias_row` record.
///
/// Spec §7.5. Record length 24 B.
///
/// ```text
/// Offsets are row-relative; the `01 00 04 00` marker sits at row offset 4. The lead is a u32 whose low byte is `0x01`, exact `0x8e`, or exact `0x8f`. The low 24 bits of `tag` are the persistent roster tag and the high byte remains part of the stored word.
/// ```
pub(crate) mod outer_alias_row {
    /// Record length in bytes. Spec §7.5.
    pub(crate) const LEN: usize = 24;
    /// Offset of `lead` (`u32`, little-endian). Spec §7.5.
    pub(crate) const LEAD: usize = 0;
    /// Offset of `marker` (`bytes[4]`). Spec §7.5.
    pub(crate) const MARKER: usize = 4;
    /// Offset of `tag` (`u32`, little-endian). Spec §7.5.
    pub(crate) const TAG: usize = 8;
    /// Offset of `flag` (`u8`). Spec §7.5.
    pub(crate) const FLAG: usize = 12;
    /// Offset of `f1` (`bytes[3]`). Spec §7.5.
    pub(crate) const F1: usize = 13;
    /// Offset of `f2` (`u32`, little-endian). Spec §7.5.
    pub(crate) const F2: usize = 16;
    /// Offset of `f3` (`u32`, little-endian). Spec §7.5.
    pub(crate) const F3: usize = 20;
}

/// Byte offsets for the `fbb_face_row` record.
///
/// Spec §7.4. Record length 8 B.
///
/// ```text
/// The leading byte of a colour-bearing FBB marker can set bit 7 without changing its face-row role. §5.2 gives the row's marker form as `(30|b0) 04 04 ff` at stride 8.
/// ```
pub(crate) mod fbb_face_row {
    /// Record length in bytes. Spec §7.4.
    pub(crate) const LEN: usize = 8;
    /// Offset of `marker` (`bytes[4]`). Spec §7.4.
    pub(crate) const MARKER: usize = 0;
    /// Offset of `alpha` (`u8`). Spec §7.4.
    pub(crate) const ALPHA: usize = 4;
    /// Offset of `blue` (`u8`). Spec §7.4.
    pub(crate) const BLUE: usize = 5;
    /// Offset of `green` (`u8`). Spec §7.4.
    pub(crate) const GREEN: usize = 6;
    /// Offset of `red` (`u8`). Spec §7.4.
    pub(crate) const RED: usize = 7;
}
