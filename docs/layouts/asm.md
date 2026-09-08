<!-- Generated from docs/layouts/asm.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `asm` record layouts

Source of truth: [`docs/formats/asm.md`](../../docs/formats/asm.md).
Table source: `docs/layouts/asm.toml`.

Covers the two ASM stream headers and ACIS 217/218 header (§1), the SAB tag inventory (§2.1), the
fixed-size ASM topology records (§5.2, §5.3), and the analytic geometry
carriers (§6.2, §6.3) as ordered token slots. Procedural spline carriers are
variable-length token graphs and are listed under "Not tabulated".

## Composite types

| Type | Bytes | Endianness | Meaning |
| ---- | ----: | ---------- | ------- |
| `sab_ref8` | 9 | little | `BinaryFile8` reference/integer chunk: one tag byte plus an eight-byte value. |
| `sab_f64` | 9 | little | `0x06` DOUBLE tag byte plus an eight-byte IEEE float64 payload. |
| `sab_position` | 25 | little | `0x13` POSITION tag byte plus three little-endian f64 coordinates. |
| `sab_vector3d` | 25 | little | `0x14` VECTOR_3D tag byte plus three little-endian f64 components. |
| `sab_logical` | 1 | n/a | A bare `0x0A` TRUE or `0x0B` FALSE tag; the tag is the whole value. |

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `0x02` | CHAR | 1 B | unsigned 8-bit | §2.1 |
| `0x03` | SHORT | 2 B | signed 16-bit | §2.1 |
| `0x04` | LONG | variable | signed int, 32 or 64-bit per the header width | §2.1 |
| `0x05` | FLOAT | 4 B | IEEE float32 | §2.1 |
| `0x06` | DOUBLE | 8 B | IEEE float64 | §2.1 |
| `0x0A` | TRUE | 0 B | logical true; a data token, not a terminator | §2.1 |
| `0x0B` | FALSE | 0 B | logical false / sentinel | §2.1 |
| `0x0C` | ENTITY_REF | variable | RecordTable index; `ref_size` wide | §2.1 |
| `0x0D` | IDENT | variable | record/class name token (leaf); one length byte plus N name bytes | §2.1 |
| `0x0E` | SUBIDENT | variable | base-class name token; one length byte plus N name bytes | §2.1 |
| `0x11` | TERMINATOR | 0 B | end of the current top-level record | §2.1 |
| `0x13` | POSITION | 24 B | 3D point, three f64; centimetres | §2.1 |
| `0x14` | VECTOR_3D | 24 B | 3D vector, three f64 | §2.1 |
| `0x15` | ENUM_VALUE | variable | enumeration or secondary integer; `ref_size` wide | §2.1 |
| `0x16` | VECTOR_2D | 16 B | 2D (u,v) | §2.1 |
| `0x17` | INT64 | 8 B | AutoCAD int64 attribute value; eight bytes in either header width | §2.1 |

## `asmheader_binaryfile8`

Spec §1 · layout: byte offsets · size: 47 B

Dialects: `acis:asm-binaryfile-8`

Fixed prefix only. The string region and the six trailing tagged metadata fields begin at byte 47 and are a sequence, not a fixed-offset structure.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 15 | `magic` | `bytes[15]` | little | spec | ASCII `ASM BinaryFile8`. |
| 15 | 4 | `save_format_version` | `u32` | little | spec | `15..19` \| little-endian u32 ACIS save-format version (`major * 100 + minor`) |
| 19 | 12 | `zero_pad` | `bytes[12]` | little | spec | `19..31` \| zero |
| 31 | 8 | `entity_count` | `u64` | little | spec | `31..39` \| little-endian u64 entity-count word |
| 39 | 8 | `flags` | `u64` | little | spec | `39..47` \| little-endian u64 flags; bit 0 is set iff the stream carries a history partition |

## `asmheader_binaryfile4`

Spec §1 · layout: byte offsets · size: 31 B

Dialects: `acis:asm-binaryfile-4`

Fixed prefix only; the string region begins at byte 31.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 15 | `magic` | `bytes[15]` | little | spec | `0..15` \| magic `ASM BinaryFile4` |
| 15 | 4 | `save_format_version` | `u32` | little | spec | `15..19` \| little-endian u32 ACIS save-format version (`major * 100 + minor`) |
| 19 | 4 | `record_count` | `u32` | little | spec | `19..23` \| little-endian u32 record count (`0` when unwritten) |
| 23 | 4 | `entity_count` | `u32` | little | spec | `23..27` \| little-endian u32 entity count |
| 27 | 4 | `flags` | `u32` | little | spec | `27..31` \| little-endian u32 flags; bit 0 is set iff the stream carries a history partition |

## `acisheader_binaryfile4`

Spec §1 · layout: byte offsets · size: 31 B

Dialects: `acis:save-format-217`, `acis:save-format-218`, `acis:save-format-binary-other`

Fixed 32-bit ACIS prefix; the tagged string region begins at byte 31.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 15 | `magic` | `bytes[15]` | little | spec | `0..15` \| magic `ACIS BinaryFile` |
| 15 | 4 | `save_format_version` | `u32` | little | spec | `15..19` \| little-endian u32 ACIS save-format version (`major * 100 + minor`) |
| 19 | 4 | `record_count` | `u32` | little | spec | `19..23` \| little-endian u32 record count (`0` when unwritten) |
| 23 | 4 | `entity_count` | `u32` | little | spec | `23..27` \| little-endian u32 entity count |
| 27 | 4 | `flags` | `u32` | little | spec | `27..31` \| little-endian u32 flags; bit 0 is set iff the stream carries a history partition (§3) |

## `body`

Spec §5.2 · layout: byte offsets · size: 61 B

Dialects: `acis:asm-binaryfile-8`

Offsets are record-relative from the leading `0x11`. On `BinaryFile4` streams ref/int chunks are 5 bytes and the offsets scale accordingly.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 16 | 9 | `chunk1_history_body_flags` | `sab_ref8` | little | spec | `chunk[1]` (@+16, i64) is `history / body flags` |
| 34 | 9 | `chunk3_first_lump` | `sab_ref8` | little | spec | `chunk[3]` @+34 = first_lump |
| 43 | 9 | `chunk4_first_wire` | `sab_ref8` | little | spec | `chunk[4]` @+43 = first_wire or `-1` |
| 52 | 9 | `chunk5_transform` | `sab_ref8` | little | spec | `chunk[5]` @+52 = transform or `-1` |

Unstated regions:

- `0..16` (16 B): Record head plus `chunk[0]`. The spec states no offset or meaning for this region; its extent follows from the stated `chunk[1]` offset.
- `25..34` (9 B): `chunk[2]`. Unnamed in the spec; the extent follows from the 9-byte chunk stride between the stated `chunk[1]` and `chunk[3]` offsets.

## `lump`

Spec §5.2 · layout: byte offsets · size: 61 B

`chunk[0]` is the attribute-chain head and `chunk[3]` is the next sibling lump; the spec states no offset for either.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 27 | 9 | `reserved_slot` | `sab_ref8` | little | spec | The @+27 slot is reserved `-1`, not the first shell |
| 43 | 9 | `chunk4_first_shell` | `sab_ref8` | little | spec | `chunk[4]` @+43 = first_shell |
| 52 | 9 | `chunk5_owner_body` | `sab_ref8` | little | spec | `chunk[5]` @+52 = owner_body |

Unstated regions:

- `0..27` (27 B): Record head, `chunk[0]` attribute-chain head, and further unnamed slots.
- `36..43` (7 B): Seven bytes between the stated `@+27` reserved slot and the stated `@+43` first_shell. This is not a multiple of the 9-byte chunk stride, so one of the two stated offsets is inconsistent with the other; the spec does not say which.

## `shell`

Spec §5.2 · layout: byte offsets · size: 80 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 53 | 9 | `chunk5_first_face` | `sab_ref8` | little | spec | `chunk[5]` @+53 = first_face |
| 62 | 9 | `chunk6_wire` | `sab_ref8` | little | derived | Offset derived from the stated `chunk[5]` @+53 plus the 9-byte chunk stride. |
| 71 | 9 | `chunk7_owner` | `sab_ref8` | little | derived | Offset derived from the stated `chunk[5]` @+53 plus two 9-byte chunk strides; it closes the declared 80-byte record exactly. |

Unstated regions:

- `0..53` (53 B): Record head, `chunk[0]` attribute-chain head, and `chunk[1..=4]`. The spec names `chunk[0]` and `chunk[3]` but states no offsets.

## `face`

Spec §5.2 · layout: byte offsets · size: 81 B

Single-sided faces end after `sides`. A double-sided face carries one further chunk, `+81 chunk[10] containment`, which is outside this fixed 81-byte extent.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 16 | 9 | `chunk1_history_face_flags` | `sab_ref8` | little | spec | +16 chunk[1] history / face flags |
| 34 | 9 | `chunk3_next_face` | `sab_ref8` | little | spec | +34 chunk[3] next_face |
| 43 | 9 | `chunk4_first_loop` | `sab_ref8` | little | spec | +43 chunk[4] first_loop |
| 52 | 9 | `chunk5_owner_shell` | `sab_ref8` | little | spec | +52 chunk[5] owner_shell |
| 70 | 9 | `chunk7_surface` | `sab_ref8` | little | spec | +70 chunk[7] surface REF |
| 79 | 1 | `chunk8_sense` | `enum8` | little | spec | +79 chunk[8] sense (0x0a=reversed, 0x0b=forward) |
| 80 | 1 | `chunk9_sides` | `enum8` | little | spec | +80 chunk[9] sides (0x0b=single) |

Unstated regions:

- `0..16` (16 B): Record head and `chunk[0]`; the spec states no offsets for them.
- `25..34` (9 B): `chunk[2]`. Unnamed in the spec; the extent follows from the 9-byte chunk stride between the stated `chunk[1]` and `chunk[3]` offsets.
- `61..70` (9 B): `chunk[6]`. Unnamed in the spec; the extent follows from the stated `chunk[5]` @+52 and `chunk[7]` @+70 offsets.

## `coedge`

Spec §5.2 · layout: byte offsets · size: 100 B

`tcoedge` inherits this complete base field sequence and appends extension chunks that do not change these offsets.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 35 | 9 | `chunk3_next_coedge` | `sab_ref8` | little | spec | +35 chunk[3] next_coedge |
| 44 | 9 | `chunk4_prev_coedge` | `sab_ref8` | little | spec | +44 chunk[4] prev_coedge |
| 53 | 9 | `chunk5_partner_coedge` | `sab_ref8` | little | spec | +53 chunk[5] partner_coedge |
| 62 | 9 | `chunk6_edge` | `sab_ref8` | little | spec | +62 chunk[6] edge |
| 71 | 1 | `chunk7_sense` | `enum8` | little | spec | +71 chunk[7] sense byte |
| 72 | 9 | `chunk8_owner_loop` | `sab_ref8` | little | spec | +72 chunk[8] owner_loop |
| 81 | 9 | `chunk9_reserved` | `sab_ref8` | little | spec | +81 chunk[9] reserved int (const 0) |
| 90 | 9 | `chunk10_pcurve` | `sab_ref8` | little | spec | +90 chunk[10] pcurve ref (or -1) |

Unstated regions:

- `0..35` (35 B): Record head and `chunk[0..=2]`; the spec states no offsets for them.

**Discrepancies:**

- The stated offsets end `chunk[10]` at +99 but the heading declares 100 B. Shifting every stated offset by +1 (head 9 bytes rather than 8, matching the `coedge` name-token length) closes the record at 100; so does leaving the offsets alone and declaring 99 B. The spec does not say which side is wrong.

## `edge`

Spec §5.2 · layout: byte offsets · size: 98 B

`tedge` carries this complete base field sequence followed by extension chunks that do not change these offsets.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 34 | 9 | `chunk3_start_vertex` | `sab_ref8` | little | spec | +34 chunk[3] start_vertex |
| 43 | 9 | `chunk4_t_start` | `sab_f64` | little | spec | +43 chunk[4] t_start (f64) |
| 52 | 9 | `chunk5_end_vertex` | `sab_ref8` | little | spec | +52 chunk[5] end_vertex |
| 61 | 9 | `chunk6_t_end` | `sab_f64` | little | spec | +61 chunk[6] t_end (f64) |
| 70 | 9 | `chunk7_owner_coedge` | `sab_ref8` | little | spec | +70 chunk[7] owner_coedge |
| 79 | 9 | `chunk8_curve` | `sab_ref8` | little | spec | +79 chunk[8] curve ref |
| 89 | 1 | `chunk9_sense` | `enum8` | little | spec | +89 chunk[9] sense byte |
| 90 | 9 | `chunk10_continuity` | `bytes[9]` | little | derived | Width derived from the `0x07` tag encoding (tag byte, one length byte, N text bytes) and the two stated seven-character literals. |

Unstated regions:

- `0..34` (34 B): Record head and `chunk[0..=2]`; the spec states no offsets for them.
- `88..89` (1 B): One byte between the end of the stated `chunk[8]` curve ref at +88 and the stated `chunk[9]` sense byte at +89. Every other chunk in this record abuts its neighbour, so one of the two stated offsets is off by one.

**Discrepancies:**

- The stated offsets end the continuity text at +99 but the heading declares 98 B. Removing the one-byte gap at +88 (placing the sense byte at +88 and the text at +89) closes the record at 98 exactly, which suggests both trailing offsets are one too high. The spec does not state which.

## `vertex`

Spec §5.2 · layout: byte offsets · size: 63 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 36 | 9 | `chunk3_owning_edge` | `sab_ref8` | little | spec | `chunk[3]` @+36 = owning_edge |
| 45 | 9 | `chunk4_index_flag` | `sab_ref8` | little | spec | `chunk[4]` @+45 = index_flag (`0` = this is the owning edge's START vertex, `1` = its END vertex) |
| 54 | 9 | `chunk5_point` | `sab_ref8` | little | spec | `chunk[5]` @+54 = point ref |

Unstated regions:

- `0..36` (36 B): Record head and `chunk[0..=2]`; the spec states no offsets for them.

## `point`

Spec §5.3 · layout: byte offsets · size: 60 B

Dialects: `acis:asm-binaryfile-8`

The record terminates immediately after the position and carries no trailing reference-count integer.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `record_head` | `bytes[8]` | little | spec | the 8-byte record head |
| 8 | 9 | `entity_base_0` | `sab_ref8` | little | derived | First of the three stated 9-byte entity-base fields, placed after the stated 8-byte head. |
| 17 | 9 | `entity_base_1` | `sab_ref8` | little | derived | Second of the three stated 9-byte entity-base fields. |
| 26 | 9 | `entity_base_2` | `sab_ref8` | little | derived | Third of the three stated 9-byte entity-base fields. |
| 35 | 25 | `position` | `sab_position` | little | spec | one 25-byte model-space `POSITION` |

## `plane_surface`

Spec §6.2 · layout: ordered slots (no stated byte offsets) · size: not stated

Total record size is not stated. Evaluation `S(u,v) = origin + u·u_dir + v·v_dir`, `v_dir = normal × u_dir`.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `origin` | `sab_position` | little | spec | **`plane`**: origin (`0x13`) |
| 1 | `normal` | `sab_vector3d` | little | spec | unit normal (`0x14`) |
| 2 | `u_dir` | `sab_vector3d` | little | spec | unit UV-reference direction (`0x14`) |

## `cone_surface`

Spec §6.2 · layout: ordered slots (no stated byte offsets) · size: 161 B

Covers circular and elliptical cylinders when the stored sine is zero. The spec gives the token order but no offsets, and no field is stated before `origin`, so the 161-byte total cannot be tiled from the listed slots alone.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `origin` | `sab_position` | little | spec | order: origin (`0x13`) |
| 1 | `axis` | `sab_vector3d` | little | spec | axis (`0x14`) |
| 2 | `ref_times_r_major` | `sab_vector3d` | little | spec | `ref × r_major` (`0x14`, magnitude = base major radius) |
| 3 | `ratio` | `sab_f64` | little | spec | `ratio = r_minor/r_major` (f64, 1.0 = circular) |
| 4 | `flag_pair` | `bytes[2]` | little | spec | Two FALSE tokens. |
| 5 | `sine_half_angle` | `sab_f64` | little | spec | `sin(half_angle)` (f64, 0 ⇒ cylinder) |
| 6 | `cosine_half_angle` | `sab_f64` | little | spec | `cos(half_angle)` (f64) |
| 7 | `u_scale` | `sab_f64` | little | spec | `u_scale` u-parameter scale (f64) |
| 8 | `trailing_flags` | `bytes[5]` | little | spec | 5×`0x0b` |

## `sphere_surface`

Spec §6.2 · layout: ordered slots (no stated byte offsets) · size: 134 B

A negative radius identifies an inward-facing, concave feature; the sign is part of the carrier. The spec states no token tag for `dir1` and `dir2`, so their widths are unknown and the 134-byte total cannot be tiled.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `center` | `sab_position` | little | spec | center (`0x13`) |
| 1 | `radius` | `sab_f64` | little | spec | **signed** radius (f64) |
| 2 | `dir1_equator` | `subrecord` | little | spec | Token tag not stated. |
| 3 | `dir2_polar_axis` | `subrecord` | little | spec | Token tag not stated. |

## `torus_surface`

Spec §6.2 · layout: ordered slots (no stated byte offsets) · size: 142 B

The 142-byte form ends at the range flag `0x0b`. A `0x0a` range flag selects the 160-byte variant, which appends start and end angles. `minor < 0` with `|minor| ≤ |major|` describes an apple/lemon torus.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `origin` | `sab_position` | little | spec | origin, axis, `major_radius` (f64) |
| 1 | `axis` | `sab_vector3d` | little | spec | origin, axis, `major_radius` (f64) |
| 2 | `major_radius` | `sab_f64` | little | spec | `major_radius` (f64) |
| 3 | `minor_radius` | `sab_f64` | little | spec | **signed** `minor_radius` (f64) |
| 4 | `ref_direction` | `sab_vector3d` | little | spec | Token tag not stated; typed here as the vector token used by the other frame directions. |
| 5 | `range_flag` | `sab_logical` | little | spec | then a range flag (`0x0b` = full 142-B variant; `0x0a` = 160-B variant with start/end angles) |

## `straight_curve`

Spec §6.3 · layout: ordered slots (no stated byte offsets) · size: 115 B

Curve range is unbounded; the owning edge's `t_start`/`t_end` clip it. The direction's magnitude is the line's parameter scale and is not necessarily 1.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `base_point` | `sab_position` | little | spec | base point + direction vector |
| 1 | `direction` | `sab_vector3d` | little | spec | base point + direction vector |

## `ellipse_curve`

Spec §6.3 · layout: ordered slots (no stated byte offsets) · size: 130 B

The 130-byte form omits the trailing start and end angles the 148-byte form carries. Circle when `ratio == 1`. The spec states no token tags for these slots.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `center` | `sab_position` | little | spec | center, axis normal, `ref × r_major` (magnitude = major radius) |
| 1 | `axis_normal` | `sab_vector3d` | little | spec | center, axis normal, `ref × r_major` (magnitude = major radius) |
| 2 | `ref_times_r_major` | `sab_vector3d` | little | spec | `ref × r_major` (magnitude = major radius) |
| 3 | `ratio` | `sab_f64` | little | spec | `ratio = r_minor/r_major` |

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| Procedural intcurves and spline surfaces (§6.3-§6.6) | §6.3 | Variable-length token graphs with recursive subtypes, revision gates, and conditional members. The spec states slot order in prose but no widths, offsets, or totals, so no arithmetic can close. |
| ASM header trailing metadata sequence (§1) | §1 | The spec states outright that the remaining header is a sequence rather than a fixed-offset structure. |
| Transform record (§5.2) | §5.2 | The spec writes the matrix extent as `13×f64 (@+18..117)`, which reads either as thirteen 9-byte DOUBLE chunks starting at +18 or as a byte range +18..117. Neither reading plus the three trailing enum bytes reaches the declared 142 B, so no offset row can be stated without choosing between them. |
| History partition (§3) | §3 | Linked `delta_state` records in the ordinary SAB token grammar; no fixed byte-offset structure is stated. |
