<!-- Generated from docs/layouts/sldprt.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `sldprt` record layouts

Source of truth: [`docs/formats/sldprt.md`](../../docs/formats/sldprt.md).
Table source: `docs/layouts/sldprt.toml`.

Covers the container envelopes (§1, §1.1-§1.3), the typed topology tag
inventory (§4), the entity common header (§5), the class-root directory (§6),
and the Parasolid geometry carriers (§7.1-§7.4). §2 documents about 125 distinct ResolvedFeatures marker
layouts in prose; the fixed-offset profile, sketch-input, reference-plane,
temporary-axis, and
cosmetic-thread carrier layouts are tabulated below, and the remaining layouts
are listed under "Not tabulated" with a coverage note.

Endianness is stated per lane: §1 container words are little-endian, §4-§7
Parasolid payload words are big-endian. Where a §1 field states no endianness
the table says `unstated` and says so in the field note.

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `00 0e` | bridge | 37 B | face-use → surface link; magic at body +8; bare record length 37 | §4 |
| `00 0f` | loop head | variable | bare record length is at least 14; no magic | §4 |
| `00 10` | edge-use | 28 B | bare magic at body +8; deltas magic at body +9 with post-magic [01][hi][lo] or [hi][lo][01] cells; deltas cell 2 carries the curve and direction uses the unique same-edge 0x2b coedge | §4 |
| `00 11` | oriented coedge | 21 B | bare body has no magic; deltas refs are nine [hi][lo][01] cells and the marker follows | §4 |
| `00 12` | vertex-use | 24 B | magic at body +16 | §4 |
| `00 1d` | world point | 38 B | no magic; four references at body +6 and xyz as three f64 BE at body +14 | §4 |

## `feature_input_shifted_scalar_trailer`

Spec §2 · layout: byte offsets · size: 35 B

The value-only scalar's fixed trailer prefix. Variable-count feature_input_operand_cell12 records follow at +35.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `zero_prefix` | `bytes[3]` | little | spec | three zero bytes |
| 3 | 4 | `object_id` | `u32` | little | spec | little-endian u32 object identifier at trailer +3 |
| 7 | 14 | `zero_object_tail` | `bytes[14]` | little | spec | fourteen zero bytes at trailer +7 |
| 21 | 6 | `layout_marker` | `bytes[6]` | little | spec | `01 00 00 00 02 00` at trailer +21 |
| 27 | 1 | `role` | `u8` | little | spec | a role byte at trailer +27 |
| 28 | 7 | `zero_tail` | `bytes[7]` | little | spec | seven zero bytes at trailer +28 |

## `feature_input_operand_cell12`

Spec §2 · layout: byte offsets · size: 12 B

Primary and legacy named-scalar operand cell. A lane-local class declaration can begin immediately after this cell.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/scalars.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `class_token` | `u16` | little | spec | little-endian u16 tag at +0 |
| 2 | 2 | `marker_address` | `u16` | little | spec | u16 marker address at +2 |
| 4 | 4 | `reference_sentinel` | `bytes[4]` | little | spec | `ff ff ff ff` at +4 |
| 8 | 4 | `zero_trailer` | `bytes[4]` | little | spec | four zero bytes at +8 |

## `outer_header`

Spec §1.1 · layout: byte offsets · size: 8 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `file_id` | `u32` | unstated | spec | The spec states the width but not the byte order of this field, unlike `version` on the same line. |
| 4 | 4 | `version` | `u32` | big | spec | `version` (u32 **big-endian**, value `0x00000004`) |

## `block_frame_header`

Spec §1.1 · layout: byte offsets · size: 26 B

Fixed prefix only. `preamble[pre_sz]` and `payload[comp_sz]` follow; the record extent is `block_end = marker_offset + 26 + pre_sz + comp_sz`.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/container.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 6 | `marker` | `bytes[6]` | little | spec | marker bytes[6] ; 14 00 06 00 08 00 · value `[20, 0, 6, 0, 8, 0]` |
| 6 | 4 | `type_id` | `u32` | little | spec | type_id u32 LE |
| 10 | 4 | `crc32` | `u32` | little | spec | crc32 u32 LE ; CRC-32 of the DECOMPRESSED payload |
| 14 | 4 | `comp_sz` | `u32` | little | spec | comp_sz u32 LE |
| 18 | 4 | `uncomp_sz` | `u32` | little | spec | uncomp_sz u32 LE |
| 22 | 4 | `pre_sz` | `u32` | little | spec | pre_sz u32 LE |

## `cache_cell_header`

Spec §1.2 · layout: byte offsets · size: 26 B

Fixed prefix only; a nibble-swapped section name of `name_len` bytes follows at +26. The three size fields are redundant scalings of one logical value `L`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 10 | 4 | `two_l` | `u32` | unstated | spec | §1.2 states the offset but not the byte order of this field. |
| 14 | 4 | `half_l` | `u32` | unstated | spec | §1.2 states the offset but not the byte order of this field. |
| 18 | 4 | `l` | `u32` | unstated | spec | §1.2 states the offset but not the byte order of this field. |
| 22 | 4 | `name_len` | `u32` | unstated | spec | §1.2 states the offset but not the byte order of this field. Valid when `0 < name_len < 500`. |

Unstated regions:

- `0..10` (10 B): Marker and the field between it and +10. §1.2 states the cell reuses the outer marker but gives no offsets in this region.

## `tail_directory_entry`

Spec §1.3 · layout: byte offsets · size: 40 B

Fixed prefix only. `name[name_len]` follows at +40 and a 6-byte trailer follows the name.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 6 | `marker` | `bytes[6]` | little | spec | +0 marker bytes[6] ; 14 00 06 00 08 00 |
| 6 | 4 | `type_id` | `u32` | little | spec | +6 type_id u32 LE |
| 10 | 4 | `zero_at_10` | `u32` | little | spec | +10 zero u32 LE |
| 14 | 4 | `size` | `u32` | little | spec | +14 size u32 LE ; section's stored/uncompressed size |
| 18 | 4 | `zero_at_18` | `u32` | little | spec | +18 zero u32 LE |
| 22 | 4 | `name_len` | `u32` | little | spec | +22 name_len u32 LE |
| 26 | 14 | `descriptor` | `bytes[14]` | little | spec | +26 descriptor bytes[14] |

## `zlb_wrapper_header`

Spec §1 · layout: byte offsets · size: 24 B

Fixed prefix only; the zlib member follows and an 8-byte trailer closes the wrapper.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/container.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `magic` | `bytes[16]` | little | spec | The wrapper is the 16-byte magic `23 1d d5 71 da 81 48 a2 a8 58 98 b2 1b 89 ef 99` · value `[35, 29, 213, 113, 218, 129, 72, 162, 168, 88, 152, 178, 27, 137, 239, 153]` |
| 16 | 4 | `uncompressed_size` | `u32` | little | spec | followed by the uncompressed byte count as u32 LE |
| 20 | 4 | `zlib_member_size` | `u32` | little | spec | the complete zlib-member byte count as u32 LE |

## `world_point`

Spec §4 · layout: byte offsets · size: 38 B

Offsets are body-relative, i.e. after the two-byte `00 1d` tag. Attrs `0` and `1` are sentinels, not world points.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 6 | 8 | `refs` | `u16[4]` | big | spec | four references at body +6 |
| 14 | 24 | `xyz` | `f64[3]` | big | spec | stores xyz as three f64 BE at body +14, in metres |

Unstated regions:

- `0..6` (6 B): §4 states no field between the body start and the reference block at +6.

## `entity_common_header`

Spec §5 · layout: byte offsets · size: 12 B

Body-relative, after the two-byte family tag. An optional `ff` byte can occur between the `00 51` tag and `flags`; it shifts every following field by one byte. Slot values follow at +12 with the schema, disc, and flo count table in specification section 5.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `flags` | `u32` | big | spec | `flo` is the low byte of `flags`. |
| 4 | 2 | `attr` | `u16` | big | spec | `attr u16 BE` |
| 6 | 4 | `seq` | `u32` | big | spec | `seq u32 BE` |
| 10 | 2 | `disc` | `u16` | big | spec | `disc u16 BE` |

## `attribute_instance_00_51`

Spec §5 · layout: byte offsets · size: 14 B

Body-relative. Node references follow from body +14 until the next record tag. The +0..+6 region is the common-header `flags` and `attr` of the same record.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 6 | 2 | `zero_selector` | `u16` | big | spec | whose u16 at body +6 is zero |
| 10 | 2 | `definition_node_id` | `u16` | big | spec | carries the u16 definition node id at body +10 |
| 12 | 2 | `owner_attribute_id` | `u16` | big | spec | and, at body +12, the attribute id of the entity it hangs on |

Unstated regions:

- `0..6` (6 B): Common-header `flags` and `attr`; §5 states no attribute-instance-specific field here.
- `8..10` (2 B): §5 states no field between the +6 selector and the +10 definition node id.

## `class_root_directory_prefix`

Spec §6 · layout: byte offsets · size: 44 B

Fixed prefix only. The root vector contains root_count u16 BE attributes from +44.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `signature` | `bytes[2]` | big | spec | +0 signature bytes[2] ; CI |
| 2 | 1 | `name_len` | `u8` | big | spec | +2 name_len u8 ; 16 |
| 3 | 16 | `field_name` | `bytes[16]` | big | spec | +3 field_name bytes[16] ; index_map_offset |
| 19 | 6 | `instance_marker` | `bytes[6]` | big | spec | +19 instance_marker bytes[6] ; 00 00 00 01 01 64 |
| 25 | 3 | `ccz` | `bytes[3]` | big | spec | +25 ccz bytes[3] ; CCZ |
| 28 | 4 | `type_tag` | `u32` | big | spec | +28 type_tag u32 BE ; 20 |
| 32 | 2 | `class_token` | `u16` | big | spec | +32 class_token u16 BE |
| 34 | 4 | `root_count` | `u32` | big | spec | +34 root_count u32 BE |
| 38 | 6 | `roots_preamble` | `bytes[6]` | big | spec | +38 roots_preamble bytes[6] ; 00 00 00 00 00 01 |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep/entity.rs` — The parser matches the fixed signature through type_tag before it reads the variable token and root count.

## `compact_analytic_header`

Spec §7.1 · layout: byte offsets · size: 17 B

Body-relative, after the two-byte `00 TT` tag and the optional `ff`. The partition form uses five u16 references and places the marker at +16; the deltas form uses five [hi][lo][01] reference triples and places it at +21. Values follow the marker in either form; `n` is the per-tag f64 count. All scalar payload values are finite, and no coordinate-magnitude cutoff is part of the format. A carrier is accepted only for a unique framing.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `attr` | `u16` | big | spec | attr u16 BE |
| 2 | 4 | `ordinal` | `u32` | big | spec | ordinal u32 BE |
| 6 | 10 | `refs` | `u16[5]` | big | spec | refs u16 BE[5] |
| 16 | 1 | `marker` | `u8` | big | spec | marker u8 (0x2b\|0x2d) |

## `analytic_values_line`

Spec §7.1 · layout: ordered slots (no stated byte offsets) · size: 48 B

The six f64 BE values that follow the compact analytic header for tag `00 1e`. Lengths are metres; the direction is unit length.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `point` | `f64[3]` | big | spec | point xyz, direction xyz |
| 1 | `direction` | `f64[3]` | big | spec | point xyz, direction xyz |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep.rs` — The parser's f64 count for tag `00 1e` matches the spec table.

## `analytic_values_circle`

Spec §7.1 · layout: ordered slots (no stated byte offsets) · size: 80 B

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `center` | `f64[3]` | big | spec | center xyz, axis xyz, refdir xyz, radius |
| 1 | `axis` | `f64[3]` | big | spec | center xyz, axis xyz, refdir xyz, radius |
| 2 | `refdir` | `f64[3]` | big | spec | center xyz, axis xyz, refdir xyz, radius |
| 3 | `radius` | `f64` | big | spec | center xyz, axis xyz, refdir xyz, radius |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep.rs` — The parser's f64 count for tag `00 1f` matches the spec table.

## `analytic_values_ellipse`

Spec §7.1 · layout: ordered slots (no stated byte offsets) · size: 88 B

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `center` | `f64[3]` | big | spec | center xyz, axis xyz, refdir xyz, major r, minor r |
| 1 | `axis` | `f64[3]` | big | spec | center xyz, axis xyz, refdir xyz, major r, minor r |
| 2 | `refdir` | `f64[3]` | big | spec | center xyz, axis xyz, refdir xyz, major r, minor r |
| 3 | `major_radius` | `f64` | big | spec | center xyz, axis xyz, refdir xyz, major r, minor r |
| 4 | `minor_radius` | `f64` | big | spec | center xyz, axis xyz, refdir xyz, major r, minor r |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep.rs` — The parser's f64 count for tag `00 20` matches the spec table.

## `analytic_values_plane`

Spec §7.1 · layout: ordered slots (no stated byte offsets) · size: 72 B

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `origin` | `f64[3]` | big | spec | origin xyz, normal xyz, refdir xyz |
| 1 | `normal` | `f64[3]` | big | spec | origin xyz, normal xyz, refdir xyz |
| 2 | `refdir` | `f64[3]` | big | spec | origin xyz, normal xyz, refdir xyz |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep.rs` — The parser's f64 count for tag `00 32` matches the spec table.

## `analytic_values_cylinder`

Spec §7.1 · layout: ordered slots (no stated byte offsets) · size: 80 B

Note the slot order: the radius precedes the reference direction, unlike the circle and ellipse records.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `origin` | `f64[3]` | big | spec | origin xyz, axis xyz, radius, refdir xyz |
| 1 | `axis` | `f64[3]` | big | spec | origin xyz, axis xyz, radius, refdir xyz |
| 2 | `radius` | `f64` | big | spec | origin xyz, axis xyz, radius, refdir xyz |
| 3 | `refdir` | `f64[3]` | big | spec | origin xyz, axis xyz, radius, refdir xyz |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep.rs` — The parser's f64 count for tag `00 33` matches the spec table.

## `analytic_values_cone`

Spec §7.1 · layout: ordered slots (no stated byte offsets) · size: 96 B

The cone fields satisfy `sin² + cos² = 1`.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `origin` | `f64[3]` | big | spec | origin xyz, axis xyz, radius, sin half-angle, cos half-angle, refdir xyz |
| 1 | `axis` | `f64[3]` | big | spec | origin xyz, axis xyz, radius, sin half-angle, cos half-angle, refdir xyz |
| 2 | `radius` | `f64` | big | spec | origin xyz, axis xyz, radius, sin half-angle, cos half-angle, refdir xyz |
| 3 | `sin_half_angle` | `f64` | big | spec | origin xyz, axis xyz, radius, sin half-angle, cos half-angle, refdir xyz |
| 4 | `cos_half_angle` | `f64` | big | spec | origin xyz, axis xyz, radius, sin half-angle, cos half-angle, refdir xyz |
| 5 | `refdir` | `f64[3]` | big | spec | origin xyz, axis xyz, radius, sin half-angle, cos half-angle, refdir xyz |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep.rs` — The parser's f64 count for tag `00 34` matches the spec table.

## `analytic_values_sphere`

Spec §7.1 · layout: ordered slots (no stated byte offsets) · size: 80 B

Note the slot order: the radius sits between the centre and the axis, unlike every other analytic record.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `center` | `f64[3]` | big | spec | center xyz, radius, axis xyz, refdir xyz |
| 1 | `radius` | `f64` | big | spec | center xyz, radius, axis xyz, refdir xyz |
| 2 | `axis` | `f64[3]` | big | spec | center xyz, radius, axis xyz, refdir xyz |
| 3 | `refdir` | `f64[3]` | big | spec | center xyz, radius, axis xyz, refdir xyz |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep.rs` — The parser's f64 count for tag `00 35` matches the spec table.

## `analytic_values_torus`

Spec §7.1 · layout: ordered slots (no stated byte offsets) · size: 88 B

A torus major radius is nonzero and its minor radius is positive.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `center` | `f64[3]` | big | spec | center xyz, axis xyz, major r, minor r, refdir xyz |
| 1 | `axis` | `f64[3]` | big | spec | center xyz, axis xyz, major r, minor r, refdir xyz |
| 2 | `major_radius` | `f64` | big | spec | center xyz, axis xyz, major r, minor r, refdir xyz |
| 3 | `minor_radius` | `f64` | big | spec | center xyz, axis xyz, major r, minor r, refdir xyz |
| 4 | `refdir` | `f64[3]` | big | spec | center xyz, axis xyz, major r, minor r, refdir xyz |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep.rs` — The parser's f64 count for tag `00 36` matches the spec table.

## `bspline_surface_descriptor`

Spec §7.2 · layout: byte offsets · size: 42 B

Body-relative after the two-byte tag and optional marker. Offsets are relative to the descriptor attribute at +0; the terminal array references occupy +32..+41. A referenced knot array may have trailing physical entries beyond its distinct-knot count when every matching trailing multiplicity is zero; those f64 slots are ignored.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `attr` | `u16` | big | spec | descriptor attribute |
| 2 | 1 | `u_periodic` | `u8` | big | spec | `u_periodic` logical byte |
| 3 | 1 | `v_periodic` | `u8` | big | spec | `v_periodic` logical byte |
| 4 | 2 | `u_degree` | `u16` | big | spec | u_degree |
| 6 | 2 | `v_degree` | `u16` | big | spec | v_degree |
| 8 | 4 | `u_pole_count` | `u32` | big | spec | u_pole_count |
| 12 | 4 | `v_pole_count` | `u32` | big | spec | v_pole_count |
| 16 | 1 | `u_knot_type` | `u8` | big | spec | u_knot_type |
| 17 | 1 | `v_knot_type` | `u8` | big | spec | v_knot_type |
| 18 | 4 | `u_distinct_knot_count` | `u32` | big | spec | u_distinct_knot_count |
| 22 | 4 | `v_distinct_knot_count` | `u32` | big | spec | v_distinct_knot_count |
| 26 | 1 | `rational` | `u8` | big | spec | `rational` logical byte |
| 27 | 1 | `u_closed` | `u8` | big | spec | `u_closed` logical byte |
| 28 | 1 | `v_closed` | `u8` | big | spec | `v_closed` logical byte |
| 29 | 1 | `surface_form` | `u8` | big | spec | surface_form |
| 30 | 2 | `vertex_dim` | `u16` | big | spec | vertex_dim |
| 32 | 10 | `array_refs` | `u16[5]` | big | spec | terminal u16 BE refs |

## `bspline_array_header`

Spec §7.2 · layout: byte offsets · size: 6 B

Shared header of `00 2d` (poles, f64 elements), `00 7f` (knot multiplicities, u16 elements), and `00 80` (unique knot values, f64 elements). Offsets are relative to the byte after the tag and the marker. Element data follows at +6. A surface knot array may include trailing physical entries beyond the descriptor count when every matching trailing multiplicity is zero; the extra f64 slots are ignored.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `count` | `u32` | big | spec | value_count u32 BE |
| 4 | 2 | `attr` | `u16` | big | spec | attr u16 BE |

## `bspline_compact_array_header`

Spec §7.2 · layout: byte offsets · size: 4 B

Complete compact-array header including its leading zero byte. Element data follows at +4. The referencing descriptor role selects f64 control/knot values or u16 multiplicities.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/brep/spline.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `zero` | `u8` | big | spec | 00 count:u8 |
| 1 | 1 | `count` | `u8` | big | spec | count:u8 |
| 2 | 2 | `attr` | `u16` | big | spec | attr u16 BE |

## `intersection_composite`

Spec §7.3 · layout: byte offsets · size: 29 B

Body-relative, after the two-byte `00 26` tag. The intersection-data form replaces the tag with `00 01 5a` and keeps this layout unchanged.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `attr` | `u16` | big | spec | 00 26 attr u16 BE ordinal u32 BE |
| 2 | 4 | `ordinal` | `u32` | big | spec | ordinal u32 BE refs u16 BE[5] |
| 6 | 10 | `refs` | `u16[5]` | big | spec | refs u16 BE[5] marker u8 (0x2b\|0x2d) |
| 16 | 1 | `marker` | `u8` | big | spec | marker u8 (0x2b\|0x2d) |
| 17 | 12 | `payload` | `u16[6]` | big | spec | payload u16 BE[6] = [support0, support1, chart, term_start, term_end, uv] |

## `chart_00_28`

Spec §7.3 · layout: byte offsets · size: 52 B

Offsets are body-relative as the spec writes them. `count` point entries follow at +52; an entry is either 88 bytes (point xyz, then a finite nonzero tangent at entry +56) or a bare 24-byte point. The chart stores no stride discriminator; chart, terminator, and optional support-UV witnesses must select exactly one entry width.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `count` | `u32` | big | spec | `count u32 BE, attr u16 BE, base_parameter f64 BE |
| 4 | 2 | `attr` | `u16` | big | spec | attr u16 BE, base_parameter f64 BE |
| 6 | 8 | `base_parameter` | `f64` | big | derived | Offset derived by laying the spec's ordered field list out from the body start. |
| 14 | 8 | `base_scale` | `f64` | big | derived | Offset derived by laying the spec's ordered field list out from the body start. |
| 22 | 4 | `chart_count` | `u32` | big | derived | Offset derived by laying the spec's ordered field list out from the body start. |
| 26 | 8 | `chordal_error` | `f64` | big | derived | Offset derived by laying the spec's ordered field list out from the body start. |
| 34 | 8 | `unnamed_f64` | `f64` | big | derived | Offset derived by laying the spec's ordered field list out from the body start. The spec gives this slot no name. |
| 36 | 8 | `absent_sentinel_a` | `f64` | big | spec | two absent-value sentinels `-3.14158e13` at body +36 and +44 |
| 44 | 8 | `absent_sentinel_b` | `f64` | big | spec | at body +36 and +44, then `count` point entries at body +52 |

**Discrepancies:**

- The spec's ordered field list places the unnamed seventh f64 at body +34..+42, which overlaps the sentinel the same paragraph puts at body +36. The two are consistent only if the stated `+36`, `+44`, and `+52` are measured from the byte after `attr` (body +6) rather than from the body start; `crates/cadmpeg-codec-sldprt/src/brep/intersection.rs` reads them that way, placing the sentinels at body +42 and +50 and the point block at body +58. §4 uses body-relative offsets elsewhere (`00 1d` xyz at body +14), so the two conventions collide inside one document. The spec does not say which applies here.

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep/intersection.rs` — The parser anchors the point block 52 bytes after `preamble = body + 6`, not after the body start; this is the discrepancy recorded above.

## `support_uv_00_cc`

Spec §7.3 · layout: byte offsets · size: 7 B

Body-relative fixed prefix; f64 BE values follow at +7, `width` per chart point.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `count` | `u32` | big | spec | `count u32 BE, attr u16 BE, width u8 (2\|3\|4)` |
| 4 | 2 | `attr` | `u16` | big | spec | attr u16 BE, width u8 (2\|3\|4) |
| 6 | 1 | `width` | `u8` | big | spec | The spec admits `3`; the parser maps every non-`4` marker to stride 2, so a stored `3` is decoded as stride 2. Recorded as a discrepancy in the pull request; the table states the spec's value set. |

## `rolling_ball_blend_00_38`

Spec §7.4 · layout: byte offsets · size: 56 B

Body-relative, after the two-byte tag and the optional `ff`. `abs(offset0) == abs(offset1) > 0`; their common magnitude is the constant rolling-ball radius. Each `side` value is exactly `+1` or `-1`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `attr` | `u16` | big | spec | 00 38 [ff]? attr u16 BE ordinal u32 BE |
| 2 | 4 | `ordinal` | `u32` | big | spec | ordinal u32 BE refs u16 BE[5] |
| 6 | 10 | `refs` | `u16[5]` | big | spec | refs u16 BE[5] |
| 16 | 1 | `marker` | `u8` | big | spec | marker u8 (0x2b\|0x2d) |
| 17 | 1 | `selector` | `u8` | big | spec | The spec writes the two values without a `0x` prefix on the line above the hex-prefixed marker; the parser matches `0x45` and `0x52`. Recorded as a discrepancy in the pull request. |
| 18 | 2 | `support0` | `u16` | big | spec | support0 u16 BE support1 u16 BE spine u16 BE |
| 20 | 2 | `support1` | `u16` | big | spec | support1 u16 BE spine u16 BE |
| 22 | 2 | `spine` | `u16` | big | spec | spine u16 BE |
| 24 | 8 | `offset0` | `f64` | big | spec | offset0 f64 BE offset1 f64 BE side0 f64 BE side1 f64 BE |
| 32 | 8 | `offset1` | `f64` | big | spec | offset1 f64 BE side0 f64 BE side1 f64 BE |
| 40 | 8 | `side0` | `f64` | big | spec | side0 f64 BE side1 f64 BE |
| 48 | 8 | `side1` | `f64` | big | spec | side1 f64 BE |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep/blend.rs` — The parser's selector value set; see the `selector` field note.

## `offset_surface_00_3c`

Spec §7.5 · layout: byte offsets · size: 29 B

Partition body relative to the byte after the `00 3c` tag and optional `ff`. With the two-byte tag, the compact record is 31 bytes. The deltas form expands each reference, including `support`, to `[hi][lo][01]`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `attr` | `u16` | big | spec | attr u16 BE ordinal u32 BE |
| 2 | 4 | `ordinal` | `u32` | big | spec | ordinal u32 BE refs u16 BE[5] |
| 6 | 10 | `refs` | `u16[5]` | big | spec | refs u16 BE[5] marker u8 (0x2b\|0x2d) |
| 16 | 1 | `marker` | `u8` | big | spec | marker u8 (0x2b\|0x2d) |
| 17 | 1 | `discriminator` | `u8` | big | spec | discriminator u8 ('V'\|'I'\|'U') |
| 18 | 1 | `true_offset` | `u8` | big | spec | true_offset u8 (0\|1) |
| 19 | 2 | `support` | `u16` | big | spec | support u16 BE distance f64 BE |
| 21 | 8 | `distance` | `f64` | big | spec | distance f64 BE |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/brep/offset.rs` — The parser identifies the type-60 carrier by its exact tag.

## `extended_wide_104_profile_curve`

Spec §2 · layout: byte offsets · size: 104 B

The endpoint fields are zero-based ordinals in the feature-owned coordinate-bearing geometry roster.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | An extended-prefix 104-byte wide indexed profile curve |
| 17 | 4 | `native_kind` | `u32` | little | spec | kind u32 `0`, `1`, or `2` |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 31 | 8 | `profile_selector` | `bytes[8]` | little | spec | `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 8 | `zero_endpoint_prefix` | `bytes[8]` | little | spec | zero bytes at marker +56 through +63 and +80 through +87 |
| 64 | 2 | `endpoint_first` | `u16` | little | spec | zero-based coordinate-roster endpoint ordinals at marker +64 and +66 |
| 66 | 2 | `endpoint_second` | `u16` | little | spec | zero-based coordinate-roster endpoint ordinals at marker +64 and +66 |
| 68 | 4 | `endpoint_selector` | `u32` | little | spec | u32 `1` at marker +68 |
| 72 | 8 | `signed_selector` | `f64` | little | spec | f64 `-1` at marker +72 |
| 80 | 8 | `zero_trailer_prefix` | `bytes[8]` | little | spec | zero bytes at marker +56 through +63 and +80 through +87 |
| 88 | 4 | `trailer_tag0` | `bytes[4]` | little | spec | `00 00 01 00` at marker +88 and +92 |
| 92 | 4 | `trailer_tag1` | `bytes[4]` | little | spec | `00 00 01 00` at marker +88 and +92 |
| 96 | 4 | `zero_trailer_suffix` | `bytes[4]` | little | spec | zero bytes at marker +96 through +99 |
| 100 | 4 | `identity` | `u32` | little | spec | u32 `1` at marker +100 |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this profile layout.
- `21..23` (2 B): The profile locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `legacy_wide_112_profile_roster_curve`

Spec §2 · layout: byte offsets · size: 112 B

The endpoint fields are zero-based ordinals in the complete feature-owned coordinate-bearing geometry roster; this state-zero trailer is distinct from the object-index wide curve trailer.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/endpoints.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | A legacy-prefix 112-byte profile-roster curve |
| 17 | 4 | `native_kind` | `u32` | little | spec | native kind u32 `0`, `1`, or `2` |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 8 | `zero_endpoint_prefix` | `bytes[8]` | little | spec | eight zero bytes at marker +56 |
| 64 | 2 | `endpoint_first` | `u16` | little | spec | zero-based coordinate-roster endpoint ordinals at marker +64 and +66 |
| 66 | 2 | `endpoint_second` | `u16` | little | spec | zero-based coordinate-roster endpoint ordinals at marker +64 and +66 |
| 68 | 4 | `endpoint_selector` | `u32` | little | spec | Marker +68 stores u32 `1` |
| 72 | 8 | `signed_selector` | `f64` | little | spec | marker +72 stores f64 `-1` |
| 80 | 4 | `trailer_selector` | `i32` | little | spec | marker +80 stores selector i32 `-1` or `1` |
| 84 | 2 | `local_state` | `u16` | little | spec | marker +84 stores zero u16 |
| 86 | 16 | `reference_sentinels` | `i32[4]` | little | spec | Four i32 `-2` reference sentinels occupy marker +86 through +101 |
| 102 | 2 | `zero_trailer` | `bytes[2]` | little | spec | marker +102 stores zero u16 |
| 104 | 4 | `identity_first` | `u32` | little | spec | distinct non-sentinel u32 identities occupy marker +104 and +108 |
| 108 | 4 | `identity_second` | `u32` | little | spec | distinct non-sentinel u32 identities occupy marker +104 and +108 |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this profile layout.
- `21..23` (2 B): The profile locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `legacy_wide_104_profile_roster_curve`

Spec §2 · layout: byte offsets · size: 104 B

The endpoint fields are zero-based ordinals in the complete feature-owned coordinate-bearing geometry roster.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/endpoints.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | A legacy-prefix 104-byte profile-roster curve |
| 17 | 4 | `native_kind` | `u32` | little | spec | native kind u32 `0`, `1`, or `2` |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 8 | `zero_endpoint_prefix` | `bytes[8]` | little | spec | eight zero bytes at marker +56 |
| 64 | 2 | `endpoint_first` | `u16` | little | spec | zero-based coordinate-roster endpoint ordinals at marker +64 and +66 |
| 66 | 2 | `endpoint_second` | `u16` | little | spec | zero-based coordinate-roster endpoint ordinals at marker +64 and +66 |
| 68 | 4 | `endpoint_selector` | `u32` | little | spec | Marker +68 stores u32 `1` |
| 72 | 8 | `signed_selector` | `f64` | little | spec | marker +72 stores f64 `-1` |
| 80 | 8 | `zero_trailer_prefix` | `bytes[8]` | little | spec | marker +80 through +87 are zero |
| 88 | 4 | `local_id` | `u32` | little | spec | marker +88 stores the marker local identifier |
| 92 | 4 | `zero_trailer_gap` | `bytes[4]` | little | spec | marker +92 through +95 are zero |
| 96 | 4 | `trailer_tag` | `u32` | little | spec | marker +96 stores u32 `4` |
| 100 | 4 | `next_object_index` | `u32` | little | spec | marker +100 stores u32 `1` |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this profile layout.
- `21..23` (2 B): The profile locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `extended_geometry_104_indexed_arc`

Spec §2 · layout: byte offsets · size: 104 B

Distinct endpoint indices define a minor arc; equal endpoint indices use the extended full-circle layout.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | A second extended-prefix kind `0` geometry-locus 104-byte bounded-arc form |
| 17 | 4 | `native_kind` | `u32` | little | spec | kind `0` geometry-locus |
| 23 | 4 | `geometry_locus` | `bytes[4]` | little | spec | geometry-locus 104-byte bounded-arc form |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector bytes `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `endpoint_first` | `u16` | little | spec | endpoint indices at marker +56 and +58 are zero-based positions |
| 58 | 2 | `endpoint_second` | `u16` | little | spec | endpoint indices at marker +56 and +58 are zero-based positions |
| 60 | 4 | `endpoint_selector` | `u32` | little | spec | u32 `1` at marker +60 |
| 64 | 8 | `signed_radius_selector` | `f64` | little | spec | f64 `-1` at marker +64 |
| 72 | 4 | `arc_selector` | `i32` | little | spec | signed selector `1` or `-1` at marker +72 |
| 76 | 2 | `center_index` | `u16` | little | spec | u16 center index at marker +76 |
| 78 | 16 | `reference_sentinels` | `bytes[16]` | little | spec | four i32 `-2` cells |
| 94 | 2 | `terminator` | `u16` | little | spec | marker +94 is zero u16 |
| 96 | 8 | `trailer_identities` | `u32[2]` | little | spec | two u32 trailer identities are at marker +96 and +100 |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this geometry-locus layout.
- `21..23` (2 B): The geometry locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `extended_geometry_116_indexed_arc`

Spec §2 · layout: byte offsets · size: 116 B

The relation tail is bounded by the following sketch marker at +116.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | The same geometry-locus arc header has a 116-byte relation-tail form |
| 17 | 4 | `native_kind` | `u32` | little | spec | kind `0` geometry-locus |
| 23 | 4 | `geometry_locus` | `bytes[4]` | little | spec | geometry-locus 104-byte bounded-arc form |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector bytes `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `endpoint_first` | `u16` | little | spec | endpoint indices at marker +56 and +58 are zero-based positions |
| 58 | 2 | `endpoint_second` | `u16` | little | spec | endpoint indices at marker +56 and +58 are zero-based positions |
| 60 | 4 | `endpoint_selector` | `u32` | little | spec | u32 `1` at marker +60 |
| 64 | 8 | `signed_radius_selector` | `f64` | little | spec | f64 `-1` at marker +64 |
| 72 | 4 | `arc_selector` | `i32` | little | spec | signed selector `1` or `-1` at marker +72 |
| 76 | 2 | `center_index` | `u16` | little | spec | u16 center index at marker +76 |
| 78 | 16 | `reference_sentinels` | `bytes[16]` | little | spec | four i32 `-2` cells |
| 94 | 8 | `relation_tail_padding` | `bytes[8]` | little | spec | zero bytes at marker +94 through +101 |
| 102 | 4 | `relation_kind` | `u32` | little | spec | u32 `4` at marker +102 |
| 106 | 6 | `relation_padding` | `bytes[6]` | little | spec | zero bytes at marker +106 through +111 |
| 112 | 4 | `following_object_index` | `u32` | little | spec | nonzero, non-null u32 object index at marker +112 |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this geometry-locus layout.
- `21..23` (2 B): The geometry locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `compact_indexed_curve_continuation120`

Spec §2 · layout: byte offsets · size: 122 B

A valid class declaration may begin at the record boundary +122.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/endpoints.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | An extended- or current-prefix compact indexed curve |
| 17 | 4 | `native_kind` | `u32` | little | spec | kind u32 `0`, `1`, or `2` |
| 23 | 4 | `locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `endpoint_first` | `u16` | little | spec | zero-based coordinate-roster ordinals at marker +56 and +58 |
| 58 | 2 | `endpoint_second` | `u16` | little | spec | zero-based coordinate-roster ordinals at marker +56 and +58 |
| 60 | 4 | `endpoint_selector` | `u32` | little | spec | u32 `1` at marker +60 |
| 64 | 8 | `signed_selector` | `f64` | little | spec | f64 `-1` at marker +64 |
| 72 | 48 | `continuation_padding` | `bytes[48]` | little | spec | 48 zero bytes from marker +72 through +119 |
| 120 | 2 | `continuation_kind` | `u16` | little | spec | Marker +120 stores a nonzero, non-null u16 continuation discriminator |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this indexed curve layout.
- `21..23` (2 B): The locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `current_terminal_relation_carrier`

Spec §2 · layout: byte offsets · size: 136 B

The class declaration begins at the record boundary +136 and is owned by the matching feature relation.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/endpoints.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | A current-prefix terminal relation carrier |
| 17 | 4 | `native_kind` | `u32` | little | spec | kind u32 `2` |
| 23 | 4 | `geometry_locus` | `bytes[4]` | little | spec | geometry locus `05 00 01 00` |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 8 | `zero_endpoint_prefix` | `bytes[8]` | little | spec | eight zero bytes at marker +56 |
| 64 | 4 | `terminal_header` | `bytes[4]` | little | spec | `01 00 01 00` at marker +64 |
| 68 | 4 | `endpoint_selector` | `u32` | little | spec | u32 `1` at marker +68 |
| 72 | 8 | `signed_selector` | `f64` | little | spec | f64 `-1` at marker +72 |
| 80 | 4 | `terminal_selector` | `u32` | little | spec | u32 `1` at marker +80 |
| 84 | 2 | `terminal_state` | `u16` | little | spec | zero u16 at marker +84 |
| 86 | 16 | `reference_sentinels` | `bytes[16]` | little | spec | four i32 `-2` cells at marker +86 |
| 102 | 32 | `zero_tail` | `bytes[32]` | little | spec | 32 zero bytes at marker +102 |
| 134 | 2 | `terminal_tag` | `u16` | little | spec | u16 `3` at marker +134 |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this relation carrier.
- `21..23` (2 B): The geometry locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `extended_geometry_terminal_circle_dimension_tail`

Spec §2 · layout: byte offsets · size: 160 B

The equal-index circle uses the preceding entry in the complete coordinate-bearing geometry roster as its center.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | An equal-index geometry-locus circle uses a terminal dimension tail |
| 17 | 4 | `native_kind` | `u32` | little | spec | extended-prefix kind `0` geometry-locus record |
| 23 | 4 | `geometry_locus` | `bytes[4]` | little | spec | 104-byte compact indexed layout |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values are `1` |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values are `1` |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector bytes at marker +31 are `00 00 80 bf 00 00 04 00` |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `endpoint_first` | `u16` | little | spec | Marker +56 and marker +58 contain the same nonzero one-based index in the complete coordinate-bearing geometry roster |
| 58 | 2 | `endpoint_second` | `u16` | little | spec | Marker +56 and marker +58 contain the same nonzero one-based index in the complete coordinate-bearing geometry roster |
| 60 | 4 | `endpoint_selector` | `u32` | little | spec | marker +60 stores u32 `1` |
| 64 | 8 | `signed_radius_selector` | `f64` | little | spec | marker +64 stores f64 `-1` |
| 72 | 4 | `arc_selector` | `i32` | little | spec | marker +72 stores signed selector `1` or `-1` |
| 76 | 2 | `auxiliary_index` | `u16` | little | spec | Marker +76 stores u16 auxiliary index |
| 78 | 16 | `reference_sentinels` | `bytes[16]` | little | spec | marker +78 contains four i32 `-2` cells |
| 94 | 34 | `terminal_padding` | `bytes[34]` | little | spec | marker +94 through +127 are zero |
| 128 | 2 | `dimension_kind` | `u16` | little | spec | marker +128 stores u16 `4` |
| 130 | 4 | `reference` | `u32` | little | spec | marker +130 stores a nonzero, non-null u32 reference |
| 134 | 2 | `dimension_state` | `u16` | little | spec | marker +134 stores zero u16 |
| 136 | 4 | `dimension_value` | `u32` | little | spec | marker +136 stores a nonzero, non-null u32 value |
| 140 | 8 | `dimension_suffix` | `bytes[8]` | little | spec | marker +140 through +147 stores `ff fe ff 02 44 00 31 00` |
| 148 | 8 | `trailing_value` | `f64` | little | spec | marker +148 stores f64 `2` |
| 156 | 4 | `terminal_sentinel` | `bytes[4]` | little | spec | marker +156 through +159 are `ff` |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this geometry-locus layout.
- `21..23` (2 B): The geometry locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `extended_selector44_indexed_line_continuation`

Spec §2 · layout: byte offsets · size: 84 B

The endpoint fields are zero-based indices in the feature-owned coordinate-bearing point roster.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | An extended-prefix kind-`0` selector-`44` indexed line |
| 17 | 4 | `native_kind` | `u32` | little | spec | kind-`0` selector-`44` indexed line |
| 23 | 4 | `locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` or geometry locus `05 00 01 00` |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector bytes `00 00 80 bf 00 00 44 00` at marker +31 |
| 39 | 9 | `continuation_header` | `bytes[9]` | little | spec | A continuation ending instead stores `00 00 01 00 00 00 00 00 00` at marker +39 through +47 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `endpoint_first` | `u16` | little | spec | distinct zero-based indices in the feature-owned coordinate-bearing point roster |
| 58 | 2 | `endpoint_second` | `u16` | little | spec | distinct zero-based indices in the feature-owned coordinate-bearing point roster |
| 60 | 4 | `endpoint_selector` | `u32` | little | spec | Marker +60 stores u32 `1` |
| 64 | 8 | `signed_selector` | `f64` | little | spec | marker +64 stores f64 `-1` |
| 72 | 12 | `continuation_body` | `bytes[12]` | little | spec | `00 00 01 00 02 00 00 00 02 00 00 00` at marker +72 through +83 |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this indexed-line layout.
- `21..23` (2 B): The locus begins at +23; bytes +21 through +22 are reserved.

## `extended_selector44_indexed_line_control_terminal`

Spec §2 · layout: byte offsets · size: 170 B

The endpoint fields are zero-based indices in the feature-owned coordinate-bearing point roster.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | An extended-prefix kind-`0` selector-`44` indexed line |
| 17 | 4 | `native_kind` | `u32` | little | spec | kind-`0` selector-`44` indexed line |
| 23 | 4 | `locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` or geometry locus `05 00 01 00` |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector bytes `00 00 80 bf 00 00 44 00` at marker +31 |
| 39 | 9 | `terminal_prefix` | `bytes[9]` | little | spec | Both terminal endings store zero bytes at marker +39 through +47 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `endpoint_first` | `u16` | little | spec | distinct zero-based indices in the feature-owned coordinate-bearing point roster |
| 58 | 2 | `endpoint_second` | `u16` | little | spec | distinct zero-based indices in the feature-owned coordinate-bearing point roster |
| 60 | 4 | `endpoint_selector` | `u32` | little | spec | Marker +60 stores u32 `1` |
| 64 | 8 | `signed_selector` | `f64` | little | spec | marker +64 stores f64 `-1` |
| 72 | 70 | `terminal_padding` | `bytes[70]` | little | spec | A control terminal ending stores zero bytes at marker +72 through +141 |
| 142 | 2 | `terminal_tag` | `bytes[2]` | little | spec | `08 80` at marker +142 through +143 |
| 144 | 10 | `terminal_suffix` | `bytes[10]` | little | spec | zero bytes at marker +144 through +153 |
| 154 | 16 | `control_sequence` | `bytes[16]` | little | spec | the control sequence `01 00 01 00 02 00 00 00 03 00 00 00 02 00 00 00` at marker +154 through +169 |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this indexed-line layout.
- `21..23` (2 B): The locus begins at +23; bytes +21 through +22 are reserved.

## `extended_terminal_164_wide_profile_curve`

Spec §2 · layout: byte offsets · size: 164 B

The endpoint fields are zero-based ordinals in the feature-owned coordinate-bearing geometry roster.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | An extended-prefix terminal 164-byte wide indexed profile curve |
| 17 | 4 | `native_kind` | `u32` | little | spec | kind u32 `0`, `1`, or `2` |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 29 | 2 | `state` | `u16` | little | spec | role and state u16 values `1` at marker +27 and +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 8 | `zero_endpoint_prefix` | `bytes[8]` | little | spec | zero bytes at marker +56 through +63 |
| 64 | 2 | `endpoint_first` | `u16` | little | spec | zero-based coordinate-roster endpoint ordinals at marker +64 and +66 |
| 66 | 2 | `endpoint_second` | `u16` | little | spec | zero-based coordinate-roster endpoint ordinals at marker +64 and +66 |
| 68 | 4 | `endpoint_selector` | `u32` | little | spec | u32 `1` at marker +68 |
| 72 | 8 | `signed_selector` | `f64` | little | spec | f64 `-1` at marker +72 |
| 80 | 54 | `zero_trailer` | `bytes[54]` | little | spec | zero bytes from marker +80 through +133 |
| 134 | 2 | `terminal_state` | `u16` | little | spec | u16 `3` at marker +134 |
| 136 | 8 | `zero_terminal_padding` | `bytes[8]` | little | spec | eight zero bytes at marker +136 |
| 144 | 4 | `null_identity` | `u32` | little | spec | the null u32 identity at marker +144 |
| 148 | 16 | `zero_terminal_suffix` | `bytes[16]` | little | spec | 16 zero bytes at marker +148 |

Unstated regions:

- `5..17` (12 B): The marker header does not define bytes +5 through +16 for this profile layout.
- `21..23` (2 B): The profile locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `legacy_140_single_incidence_profile_point`

Spec §2 · layout: byte offsets · size: 140 B

The record emits a point. The fixed layout includes the single-incidence and shared-f32 variants with their complete identity trailers.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | The legacy prefix has a 140-byte single-incidence profile-point record |
| 5 | 8 | `header` | `bytes[8]` | little | spec | eight `ff` bytes at marker +5 |
| 13 | 4 | `sentinel` | `f32` | little | spec | f32 `-1` at marker +13 |
| 17 | 4 | `native_kind` | `u32` | little | spec | native code u32 at marker +17 |
| 21 | 2 | `zero_prefix` | `bytes[2]` | little | spec | zero bytes at marker +21 through +22 |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role u16 `1` |
| 29 | 2 | `zero_state` | `u16` | little | spec | zero state u16 at marker +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | `00 00 80 bf 00 00 04 00` at marker +31 |
| 39 | 9 | `zero_state_prefix` | `bytes[9]` | little | spec | f64 `1` at marker +48 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `coordinate_tag` | `bytes[2]` | little | spec | coordinate tag `1e 00` at marker +56 |
| 58 | 8 | `coordinate_first` | `f64` | little | spec | finite f64 coordinates at marker +58 and +66 |
| 66 | 8 | `coordinate_second` | `f64` | little | spec | finite f64 coordinates at marker +58 and +66 |
| 74 | 2 | `zero_link_prefix` | `u16` | little | spec | Marker +74 is zero |
| 76 | 2 | `link_state` | `u16` | little | spec | native code and link-state u16 pairs `(0, 2)`, `(1, 2)`, or `(2, 1)` |
| 78 | 12 | `incidence_cell` | `bytes[12]` | little | spec | One 12-byte incidence cell at marker +78 |
| 90 | 6 | `link_terminator` | `bytes[6]` | little | spec | The terminator `fe ff ff ff 00 00` begins at marker +90 |
| 96 | 32 | `trailer_prefix` | `bytes[32]` | little | spec | zero bytes from marker +96 through +127 |
| 128 | 4 | `identity_first` | `u32` | little | spec | a nonzero, non-null u32 identity at marker +128 |
| 132 | 4 | `trailer_middle` | `bytes[4]` | little | spec | A paired trailer can also use the marker +128 identity and zero bytes at marker +132 through +135 |
| 136 | 4 | `identity_second` | `u32` | little | spec | a distinct nonzero, non-null u32 identity at marker +136 |

## `legacy_144_single_incidence_profile_point`

Spec §2 · layout: byte offsets · size: 144 B

The record emits a point. Its terminal identity and next-marker boundary are four bytes beyond the 140-byte shared-f32 form.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | 144-byte shared-f32 single-incidence profile point |
| 5 | 8 | `header` | `bytes[8]` | little | spec | eight `ff` bytes at marker +5 |
| 13 | 4 | `sentinel` | `f32` | little | spec | f32 `-1` at marker +13 |
| 17 | 4 | `native_kind` | `u32` | little | spec | native code `1` · value `1` |
| 21 | 2 | `zero_prefix` | `bytes[2]` | little | spec | zero bytes at marker +21 through +22 |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role u16 `1` · value `1` |
| 29 | 2 | `zero_state` | `u16` | little | spec | zero state u16 at marker +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | `00 00 80 bf 00 00 04 00` at marker +31 |
| 39 | 9 | `zero_state_prefix` | `bytes[9]` | little | spec | f64 `1` at marker +48 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 · value `1.0` |
| 56 | 2 | `coordinate_tag` | `bytes[2]` | little | spec | coordinate tag `1e 00` at marker +56 |
| 58 | 8 | `coordinate_first` | `f64` | little | spec | finite f64 coordinates at marker +58 and +66 |
| 66 | 8 | `coordinate_second` | `f64` | little | spec | finite f64 coordinates at marker +58 and +66 |
| 74 | 2 | `zero_link_prefix` | `u16` | little | spec | Marker +74 is zero |
| 76 | 2 | `link_state` | `u16` | little | spec | link-state u16 `1`, `2`, or `3` |
| 78 | 12 | `incidence_cell` | `bytes[12]` | little | spec | One 12-byte incidence cell at marker +78 |
| 90 | 4 | `zero_post_cell` | `bytes[4]` | little | spec | Four zero bytes occupy marker +90 through +93 |
| 94 | 6 | `link_terminator` | `bytes[6]` | little | spec | The terminator `fe ff ff ff 00 00` begins at marker +94 |
| 100 | 40 | `trailer_prefix` | `bytes[40]` | little | spec | zero bytes from marker +100 through +139 |
| 140 | 4 | `identity` | `u32` | little | spec | a nonzero, non-null u32 identity at marker +140 |

## `extended_scaled_146_profile_point`

Spec §2 · layout: byte offsets · size: 146 B

The record emits a point; its link count is twice the trailer state.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | A scaled extended-prefix 146-byte linked profile-point record |
| 5 | 8 | `header` | `bytes[8]` | little | spec | eight `ff` bytes at marker +5 |
| 13 | 4 | `sentinel` | `f32` | little | spec | the little-endian f32 `-1.0` |
| 17 | 4 | `native_kind` | `u32` | little | spec | native value u32 `0` |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role u16 `1` |
| 29 | 2 | `state_at_29` | `u16` | little | spec | zero state u16 at marker +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `coordinate_tag` | `bytes[2]` | little | spec | coordinate tag `1e 00` at marker +56 |
| 58 | 8 | `coordinate_first` | `f64` | little | spec | finite f64 coordinates at marker +58 and +66 |
| 66 | 8 | `coordinate_second` | `f64` | little | spec | finite f64 coordinates at marker +58 and +66 |
| 74 | 2 | `zero_link_prefix` | `bytes[2]` | little | spec | Marker +74 is zero |
| 76 | 2 | `link_count` | `u16` | little | spec | Marker +76 stores a link count equal to twice the u16 trailer state at marker +134 |
| 78 | 8 | `incidence_first` | `bytes[8]` | little | spec | incidence cells at marker +78 and +86 |
| 86 | 8 | `incidence_second` | `bytes[8]` | little | spec | incidence cells at marker +78 and +86 |
| 94 | 6 | `link_terminator` | `bytes[6]` | little | spec | The link terminator `00 00 fe ff ff ff` begins at marker +94 |
| 100 | 34 | `zero_trailer_prefix` | `bytes[34]` | little | spec | Bytes +100 through +133 and +136 through +141 are zero |
| 134 | 2 | `trailer_state` | `u16` | little | spec | the u16 trailer state at marker +134 |
| 136 | 6 | `zero_trailer_suffix` | `bytes[6]` | little | spec | Bytes +100 through +133 and +136 through +141 are zero |
| 142 | 4 | `identity` | `u32` | little | spec | marker +142 stores a non-null u32 identity |

Unstated regions:

- `21..23` (2 B): The profile locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `extended_four_link_state_profile_point_prefix`

Spec §2 · layout: byte offsets · size: not stated

The fixed prefix emits a point; the trailer is variable and carries a family-specific marker prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | An extended-prefix four-link-state profile point |
| 5 | 8 | `header` | `bytes[8]` | little | spec | eight `ff` bytes at marker +5 |
| 13 | 4 | `sentinel` | `f32` | little | spec | the little-endian f32 `-1.0` |
| 17 | 4 | `native_kind` | `u32` | little | spec | native value u32 `0` |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role u16 `1` |
| 29 | 2 | `state_at_29` | `u16` | little | spec | zero state u16 at marker +29 |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `coordinate_tag` | `bytes[2]` | little | spec | coordinate tag `1e 00` at marker +56 |
| 58 | 8 | `coordinate_first` | `f64` | little | spec | finite f64 coordinates at marker +58 and +66 |
| 66 | 8 | `coordinate_second` | `f64` | little | spec | finite f64 coordinates at marker +58 and +66 |
| 74 | 2 | `zero_link_prefix` | `bytes[2]` | little | spec | Marker +74 is zero |
| 76 | 2 | `link_count` | `u16` | little | spec | marker +76 stores link count `4` |
| 78 | 8 | `incidence_first` | `bytes[8]` | little | spec | incidence cells at marker +78 and +86 |
| 86 | 8 | `incidence_second` | `bytes[8]` | little | spec | incidence cells at marker +78 and +86 |
| 94 | 6 | `link_terminator` | `bytes[6]` | little | spec | The link terminator `00 00 fe ff ff ff` begins at marker +94 |
| 100 | 34 | `zero_trailer_prefix` | `bytes[34]` | little | spec | Bytes +100 through +133 are zero |
| 134 | 2 | `trailer_state` | `u16` | little | spec | marker +134 stores trailer state `2` |

Unstated regions:

- `21..23` (2 B): The profile locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `extended_geometry_locus_138_point`

Spec §2 · layout: byte offsets · size: 138 B

The finite pair is a point coordinate; the two identity words are distinct record identities.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | The extended prefix defines a 138-byte geometry-locus point |
| 5 | 8 | `header` | `bytes[8]` | little | spec | header `ff ff ff ff ff ff ff ff` at marker +5 |
| 13 | 4 | `sentinel` | `f32` | little | spec | the little-endian f32 `-1.0` |
| 17 | 4 | `native_kind` | `u32` | little | spec | native value u32 `2` |
| 23 | 4 | `geometry_locus` | `bytes[4]` | little | spec | locus `05 00 01 00` |
| 27 | 2 | `role` | `u16` | little | spec | role u16 `1` |
| 29 | 2 | `state_at_29` | `u16` | little | spec | marker +29 u16 zero |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `coordinate_tag` | `bytes[2]` | little | spec | `1e 00` at marker +56 |
| 58 | 8 | `coordinate_first` | `f64` | little | spec | finite planar coordinates at marker +58 and +66 |
| 66 | 8 | `coordinate_second` | `f64` | little | spec | finite planar coordinates at marker +58 and +66 |
| 74 | 2 | `state_at_74` | `u16` | little | spec | Marker +74 stores u16 state `0` followed by u16 link count `1` |
| 76 | 2 | `link_count` | `u16` | little | spec | u16 link count `1` |
| 78 | 4 | `zero_link_cell` | `bytes[4]` | little | spec | marker +78 through +81 are zero |
| 82 | 4 | `link_sentinel` | `i32` | little | spec | marker +82 stores i32 `-1` |
| 86 | 38 | `zero_trailer` | `bytes[38]` | little | spec | marker +86 through +123 are zero |
| 124 | 4 | `identity_first` | `u32` | little | spec | marker +124 and +128 store distinct nonzero, non-null u32 identities |
| 128 | 4 | `identity_second` | `u32` | little | spec | marker +124 and +128 store distinct nonzero, non-null u32 identities |
| 132 | 6 | `identity_terminator` | `bytes[6]` | little | spec | marker +132 stores `00 00 01 00 00 00` |

Unstated regions:

- `21..23` (2 B): The geometry locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `extended_geometry_locus_96_construction_line`

Spec §2 · layout: byte offsets · size: 96 B

The endpoint fields are direct feature-local object identifiers.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | The extended prefix defines a 96-byte geometry-locus selected construction line |
| 5 | 8 | `header` | `bytes[8]` | little | spec | header `ff ff ff ff 04 00 ff ff` at marker +5 |
| 13 | 4 | `sentinel` | `f32` | little | spec | the little-endian f32 `-1.0` |
| 17 | 4 | `native_kind` | `u32` | little | spec | native value u32 `0` |
| 23 | 4 | `geometry_locus` | `bytes[4]` | little | spec | locus `05 00 01 00` |
| 27 | 2 | `role` | `u16` | little | spec | role u16 `2` |
| 29 | 2 | `state` | `u16` | little | spec | marker +29 u16 zero |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector `00 00 80 bf 00 00 0c 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `endpoint_first` | `u16` | little | spec | distinct nonzero u16 endpoint object identifiers are at marker +56 and +58 |
| 58 | 2 | `endpoint_second` | `u16` | little | spec | distinct nonzero u16 endpoint object identifiers are at marker +56 and +58 |
| 60 | 4 | `zero_endpoint_prefix` | `bytes[4]` | little | spec | marker +60 through +63 are zero |
| 64 | 8 | `signed_selector` | `f64` | little | spec | marker +64 stores f64 `-1` |
| 72 | 8 | `zero_selector_trailer` | `bytes[8]` | little | spec | marker +72 through +79 are zero |
| 80 | 4 | `tail_tag` | `bytes[4]` | little | spec | marker +80 stores `00 00 04 00` |
| 84 | 4 | `zero_tail_prefix` | `bytes[4]` | little | spec | marker +84 through +87 are zero |
| 88 | 4 | `identity_first` | `u32` | little | spec | nonzero, non-null u32 identities occupy marker +88 and +92 |
| 92 | 4 | `identity_second` | `u32` | little | spec | nonzero, non-null u32 identities occupy marker +88 and +92 |

Unstated regions:

- `21..23` (2 B): The geometry locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `compact_legacy_84_construction_line`

Spec §2 · layout: byte offsets · size: 84 B

The endpoint fields are direct feature-local point-object identifiers.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/endpoints.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | A compact-legacy 84-byte construction line |
| 5 | 8 | `header` | `bytes[8]` | little | spec | Its header at marker +5 is either eight `ff` bytes or |
| 13 | 4 | `shared_selector` | `bytes[4]` | little | spec | A coordinate-bearing marker has the 12-byte prefix |
| 17 | 4 | `native_kind` | `u32` | little | spec | A compact-legacy 84-byte construction line has value u32 `2` · value `2` |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role u16 `2` · value `2` |
| 29 | 2 | `state_at_29` | `u16` | little | spec | zero state at marker +29 · value `0` |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | `00 00 80 bf 00 00 0c 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 · value `1.0` |
| 56 | 2 | `endpoint_first` | `u16` | little | spec | distinct nonzero u16 point-object identifiers at marker +56 and marker +58 |
| 58 | 2 | `endpoint_second` | `u16` | little | spec | distinct nonzero u16 point-object identifiers at marker +56 and marker +58 |
| 60 | 4 | `zero_endpoint_prefix` | `bytes[4]` | little | spec | Marker +60 is zero u32 · value `[0, 0, 0, 0]` |
| 64 | 8 | `signed_selector` | `f64` | little | spec | marker +64 stores f64 `-1` · value `-1.0` |
| 72 | 4 | `trailer_state` | `bytes[4]` | little | spec | State `00 00 01 00` at marker +72 pairs with zero at marker +76 |
| 76 | 4 | `identity_first` | `u32` | little | spec | State `00 00 00 00` pairs with the same nonzero, non-null u32 identity at marker +76 and marker +80 |
| 80 | 4 | `identity_second` | `u32` | little | spec | a nonzero, non-null u32 identity at marker +80 |

Unstated regions:

- `21..23` (2 B): The profile locus begins at +23; bytes +21 through +22 are reserved.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `compact_legacy_84_coordinate_roster_curve`

Spec §2 · layout: byte offsets · size: 84 B

The endpoint fields are zero-based ordinals in the complete feature-local coordinate-bearing marker roster.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/endpoints.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | An 84-byte compact-legacy roster curve |
| 5 | 8 | `header` | `bytes[8]` | little | spec | Its header at marker +5 is either eight `ff` bytes or |
| 13 | 4 | `shared_selector` | `bytes[4]` | little | spec | A coordinate-bearing marker has the 12-byte prefix |
| 17 | 4 | `native_kind` | `u32` | little | spec | An 84-byte compact-legacy roster curve uses native/role/selector combinations |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | An 84-byte compact-legacy roster curve uses native/role/selector combinations |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | An 84-byte compact-legacy roster curve uses native/role/selector combinations |
| 48 | 8 | `state_value` | `f64` | little | spec | with f64 `1` at marker +48 · value `1.0` |
| 56 | 2 | `endpoint_first` | `u16` | little | spec | Its distinct endpoint u16 values at marker +56 and +58 |
| 58 | 2 | `endpoint_second` | `u16` | little | spec | Its distinct endpoint u16 values at marker +56 and +58 |
| 60 | 4 | `zero_endpoint_prefix` | `bytes[4]` | little | spec | zero u32 at marker +60 · value `[0, 0, 0, 0]` |
| 64 | 8 | `signed_selector` | `f64` | little | spec | f64 `-1` at marker +64 · value `-1.0` |
| 72 | 4 | `trailer_state` | `bytes[4]` | little | spec | Profile curves use trailer state `00 00 00 00` |
| 76 | 4 | `identity_first` | `u32` | little | spec | selected construction curves use state `00 00 01 00` |
| 80 | 4 | `identity_second` | `u32` | little | spec | one repeated nonzero non-null identity |

Unstated regions:

- `21..23` (2 B): The profile locus begins at +23; bytes +21 through +22 are reserved.
- `29..31` (2 B): The selector begins at +31; bytes +29 through +30 are zero.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `compact_legacy_68_profile_variant_curve`

Spec §2 · layout: byte offsets · size: 68 B

The role-1 profile-body variant uses u16 24 at +33 and carries two feature-local trailer object values.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | The compact `ff ff 07 00 01` generation |
| 5 | 8 | `header` | `bytes[8]` | little | spec | Its header at marker +5 is either eight `ff` bytes |
| 13 | 4 | `native_kind` | `u32` | little | spec | stores its native value u32 at marker +13 |
| 17 | 2 | `zero_prefix` | `bytes[2]` | little | spec | bytes +17 through +18 are zero |
| 19 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` at marker +19 |
| 23 | 2 | `role` | `u16` | little | spec | role u16 at marker +23 |
| 25 | 2 | `state` | `u16` | little | spec | state u16 `1` at marker +25 |
| 27 | 4 | `zero_body_prefix` | `bytes[4]` | little | spec | bytes +27 through +30 are zero |
| 31 | 1 | `body_tag` | `u8` | little | spec | byte `04` at marker +31 |
| 32 | 1 | `body_zero` | `u8` | little | spec | zero byte at marker +32 |
| 33 | 2 | `profile_variant` | `u16` | little | spec | u16 `0` or `24` at marker +33 |
| 35 | 7 | `zero_body_suffix` | `bytes[7]` | little | spec | seven zero bytes at marker +35 through +41 |
| 42 | 2 | `endpoint_first` | `u16` | little | spec | zero-based endpoint u16 values at marker +42 and +44 |
| 44 | 2 | `endpoint_second` | `u16` | little | spec | zero-based endpoint u16 values at marker +42 and +44 |
| 46 | 4 | `selector_value` | `u32` | little | spec | u32 `1` at marker +46 |
| 50 | 8 | `signed_selector` | `f64` | little | spec | f64 `-1` at marker +50 |
| 58 | 2 | `tail_zero_first` | `u16` | little | spec | marker +58 stores zero u16 |
| 60 | 2 | `linked_object_first` | `u16` | little | spec | marker +60 stores a feature-local object u16 |
| 62 | 2 | `tail_zero_second` | `u16` | little | spec | marker +62 stores zero u16 |
| 64 | 2 | `linked_object_second` | `u16` | little | spec | marker +64 stores a feature-local object u16 |
| 66 | 2 | `tail_zero_third` | `u16` | little | spec | marker +66 stores zero u16 |

## `compact_legacy_90_geometry_line`

Spec §2 · layout: byte offsets · size: 90 B

Endpoint values are zero-based indices in the feature-owned sketch-marker roster. The terminal variant extends the fixed body to marker +138 and has the terminal suffix described by the spec.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | The compact `ff ff 07 00 01` generation |
| 5 | 8 | `header` | `bytes[8]` | little | spec | Its header at marker +5 is either eight `ff` bytes |
| 13 | 4 | `native_kind` | `u32` | little | spec | The geometry-locus 90-byte line form has native value `1` |
| 19 | 4 | `geometry_locus` | `bytes[4]` | little | spec | locus `05 00 01 00` at marker +19 |
| 23 | 2 | `role` | `u16` | little | spec | role u16 `1` |
| 25 | 2 | `state` | `u16` | little | spec | state u16 `1` |
| 31 | 11 | `body` | `bytes[11]` | little | spec | the same byte `04` body at marker +31 |
| 42 | 2 | `endpoint_first` | `u16` | little | spec | Its endpoint values are zero-based ordinals |
| 44 | 2 | `endpoint_second` | `u16` | little | spec | Its endpoint values are zero-based ordinals |
| 46 | 4 | `selector_value` | `u32` | little | spec | u32 `1` at marker +46 |
| 50 | 8 | `signed_selector` | `f64` | little | spec | f64 `-1` at marker +50 |
| 58 | 4 | `tail_value` | `u32` | little | spec | u32 `1` at marker +58 |
| 64 | 16 | `sentinel_cells` | `i32[4]` | little | spec | four consecutive i32 `-2` cells at marker +64 |
| 80 | 2 | `tail_zero_suffix` | `u16` | little | spec | zero u16 at marker +80 |
| 82 | 4 | `identity_first` | `u32` | little | spec | nonzero non-null u32 identities at marker +82 and +86 |
| 86 | 4 | `identity_second` | `u32` | little | spec | nonzero non-null u32 identities at marker +82 and +86 |

Unstated regions:

- `17..19` (2 B): The geometry-locus prefix stores zero bytes at marker +17 through +18 before the locus.
- `27..31` (4 B): The geometry-locus body stores zero bytes at marker +27 through +30 before the body tag.
- `62..64` (2 B): Bytes +62 through +63 are not interpreted by this fixed form.

## `compact_legacy_142_profile_curve`

Spec §2 · layout: byte offsets · size: 142 B

The auxiliary pair is the arc-center candidate. Equal positive endpoint radii select a minor arc; otherwise the two endpoint pairs define a line. A four-byte separator may follow the 142-byte body before the next sketch marker.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/markers.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | The compact `ff ff 07 00 01` generation |
| 5 | 8 | `header` | `bytes[8]` | little | spec | eight `ff` bytes at marker +5 |
| 13 | 4 | `shared_selector` | `f32` | little | spec | f32 `-1` at marker +13 · value `-1.0` |
| 17 | 4 | `native_kind` | `u32` | little | spec | native value u32 `2`, profile locus `04 00 02 00` · value `2` |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `role` | `u16` | little | spec | role u16 `1` · value `1` |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 · value `1.0` |
| 64 | 2 | `curve_tag` | `bytes[2]` | little | spec | Marker +64 stores `12 00`, `16 00`, or `1a 00` |
| 66 | 8 | `auxiliary_first` | `f64` | little | spec | the finite auxiliary pair is at marker +66 and +74 |
| 74 | 8 | `auxiliary_second` | `f64` | little | spec | the finite auxiliary pair is at marker +66 and +74 |
| 82 | 4 | `body_kind` | `u32` | little | spec | Marker +82 stores u32 `11` · value `11` |
| 92 | 4 | `variant` | `u32` | little | spec | marker +92 stores an opaque variant u32 |
| 96 | 8 | `start_first` | `f64` | little | spec | The finite start and end pairs are at marker +96/+104 and marker +112/+120 |
| 104 | 8 | `start_second` | `f64` | little | spec | The finite start and end pairs are at marker +96/+104 and marker +112/+120 |
| 112 | 8 | `end_first` | `f64` | little | spec | The finite start and end pairs are at marker +96/+104 and marker +112/+120 |
| 120 | 8 | `end_second` | `f64` | little | spec | The finite start and end pairs are at marker +96/+104 and marker +112/+120 |
| 138 | 4 | `identity` | `u32` | little | spec | marker +138 stores a nonzero, non-null feature-local identity |

Unstated regions:

- `21..23` (2 B): Zero bytes at marker +21 through +22.
- `29..31` (2 B): Zero bytes at marker +29 through +30.
- `39..48` (9 B): The state value begins at marker +48; bytes +39 through +47 are reserved.
- `56..64` (8 B): Zero bytes at marker +56 through +63.
- `86..92` (6 B): Zero bytes at marker +86 through +91.
- `128..138` (10 B): Zero bytes at marker +128 through +137.

## `compact_legacy_code_two_profile_point`

Spec §2 · layout: byte offsets · size: 132 B

The record emits a point and contributes its coordinate to the raw coordinate roster.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | A compact legacy code-`2` profile point is a 132-byte record |
| 5 | 8 | `header` | `bytes[8]` | little | spec | eight `ff` bytes at marker +5 |
| 13 | 4 | `native_kind` | `u32` | little | spec | native code u32 `2` at marker +13 |
| 17 | 2 | `zero_prefix` | `bytes[2]` | little | spec | zero bytes at marker +17 through +18 |
| 19 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 23 | 2 | `role` | `u16` | little | spec | role u16 `1` |
| 25 | 6 | `zero_state` | `bytes[6]` | little | spec | zero bytes at marker +25 through +30 |
| 31 | 11 | `selector` | `bytes[11]` | little | spec | selector bytes `04 00 00 00 00 00 00 00 00 00 00` at marker +31 |
| 42 | 2 | `coordinate_tag` | `bytes[2]` | little | spec | coordinate tag `1e 00` at marker +42 |
| 44 | 8 | `coordinate_first` | `f64` | little | spec | finite f64 coordinates at marker +44 and +52 |
| 52 | 8 | `coordinate_second` | `f64` | little | spec | finite f64 coordinates at marker +44 and +52 |
| 60 | 2 | `zero_link_prefix` | `bytes[2]` | little | spec | Marker +60 through +61 are zero |
| 62 | 2 | `operand_tag` | `u16` | little | spec | marker +62 stores u16 `4` |
| 64 | 8 | `operand_first` | `bytes[8]` | little | spec | two homogeneous eight-byte operand cells begin at marker +64 and +72 |
| 72 | 8 | `operand_second` | `bytes[8]` | little | spec | two homogeneous eight-byte operand cells begin at marker +64 and +72 |
| 80 | 6 | `link_terminator` | `bytes[6]` | little | spec | Marker +80 stores `00 00 fe ff ff ff` |
| 86 | 34 | `zero_trailer` | `bytes[34]` | little | spec | marker +86 through +119 are zero |
| 120 | 4 | `trailer_kind` | `u32` | little | spec | marker +120 stores u32 `2` |
| 124 | 4 | `zero_identity_prefix` | `bytes[4]` | little | spec | marker +124 through +127 are zero |
| 128 | 4 | `identity` | `u32` | little | spec | marker +128 stores the non-null feature-local object identifier |

## `compact_legacy_embedded_geometry_handle`

Spec §2 · layout: byte offsets · size: 120 B

The record contributes its coordinate to the raw coordinate roster and emits no sketch entity.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | An embedded compact legacy geometry handle is a 120-byte record |
| 5 | 8 | `header` | `bytes[8]` | little | spec | eight `ff` bytes at marker +5 |
| 13 | 4 | `native_kind` | `u32` | little | spec | native code u32 `0` |
| 17 | 2 | `zero_prefix` | `bytes[2]` | little | spec | zero bytes at marker +17 through +18 |
| 19 | 4 | `geometry_locus` | `bytes[4]` | little | spec | geometry locus `05 00 01 00` |
| 23 | 2 | `role` | `u16` | little | spec | role u16 `1` |
| 25 | 6 | `zero_state_prefix` | `bytes[6]` | little | spec | zero bytes at marker +25 through +30 |
| 31 | 11 | `selector` | `bytes[11]` | little | spec | selector bytes `05 00 00 00 00 00 00 00 00 00 00` at marker +31 |
| 42 | 2 | `coordinate_tag` | `bytes[2]` | little | spec | coordinate tag `1e 00` at marker +42 |
| 44 | 8 | `coordinate_first` | `f64` | little | spec | finite f64 coordinates at marker +44 and +52 |
| 52 | 8 | `coordinate_second` | `f64` | little | spec | finite f64 coordinates at marker +44 and +52 |
| 60 | 4 | `state` | `u32` | little | spec | Its u32 state at marker +60 is zero or nonzero and non-null |
| 64 | 6 | `zero_link_prefix` | `bytes[6]` | little | spec | marker +64 through +69 are zero |
| 70 | 4 | `link_sentinel` | `i32` | little | spec | marker +70 stores i32 `-1` |
| 74 | 42 | `zero_trailer` | `bytes[42]` | little | spec | marker +74 through +115 are zero |
| 116 | 4 | `identity` | `u32` | little | spec | marker +116 stores a non-null feature-local identity |

## `compact_legacy_terminal_diameter_circle`

Spec §2 · layout: byte offsets · size: 121 B

The radial ordinal is zero-based in the feature-owned raw coordinate roster, including coordinate-bearing geometry handles that do not emit sketch entities.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | A terminal compact legacy diameter-circle record has kind u32 `1` |
| 5 | 8 | `header` | `bytes[8]` | little | spec | Each prefix is followed by eight `ff` bytes |
| 13 | 4 | `native_kind` | `u32` | little | spec | kind u32 `1` |
| 17 | 2 | `zero_prefix` | `bytes[2]` | little | spec | Bytes +17 through +18 are zero |
| 19 | 4 | `geometry_locus` | `bytes[4]` | little | spec | geometry locus `04 00 02 00` |
| 23 | 2 | `role` | `u16` | little | spec | profile role u16 `1` |
| 25 | 2 | `state` | `u16` | little | spec | state u16 `1` |
| 27 | 4 | `zero_selector_prefix` | `bytes[4]` | little | spec | bytes +27 through +30 are zero |
| 31 | 11 | `selector` | `bytes[11]` | little | spec | selector bytes `04 00 00 00 00 00 00 00 00 00 00` at marker +31 |
| 42 | 2 | `radial_ordinal` | `u16` | little | spec | zero-based radial-roster ordinal at marker +42 |
| 44 | 2 | `radial_sentinel` | `u16` | little | spec | u16 `0` at marker +44 |
| 46 | 4 | `selector_value` | `u32` | little | spec | u32 `1` at marker +46 |
| 50 | 8 | `signed_selector` | `f64` | little | spec | f64 `-1.0` at marker +50 |
| 58 | 44 | `zero_trailer` | `bytes[44]` | little | spec | Bytes +58 through +101 are zero |
| 102 | 2 | `terminal_state` | `u16` | little | spec | u16 `3` is at marker +102 |
| 104 | 4 | `class_marker` | `bytes[4]` | little | spec | the terminal class declaration is `ff ff 01 00` |
| 108 | 2 | `class_length` | `u16` | little | spec | u16 length `11` |
| 110 | 11 | `class_name` | `bytes[11]` | little | spec | class name `sgCircleDim` at marker +110 |

## `reference_point_short_solved_cache`

Spec §2 · layout: byte offsets · size: 277 B

Offsets begin at the byte after the UTF-16LE feature name. Unlisted bytes belong to the native construction state.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `object_header` | `bytes[8]` | little | spec | At name-end +0, the object header is `00 00 00 00 00 00 00 c0` |
| 8 | 4 | `object_id` | `u32` | little | spec | name-end +8 stores the feature object ID as u32 LE |
| 12 | 4 | `zero_after_id` | `bytes[4]` | little | spec | name-end +12 stores four zero bytes |
| 227 | 16 | `zero_before_position` | `bytes[16]` | little | spec | At name-end +227 or +243, sixteen zero bytes precede the position |
| 243 | 24 | `position` | `f64[3]` | little | spec | At name-end +243 or +259, three finite f64 LE values store xyz in metres |
| 267 | 2 | `construction_form` | `u16` | little | spec | At name-end +267 or +283, a u16 LE construction form is `4` or `5` |
| 269 | 8 | `zero_trailer` | `bytes[8]` | little | spec | At name-end +269 or +285, eight zero bytes terminate the solved-position cache |

Unstated regions:

- `16..227` (211 B): Native reference-point construction state; the solved datum position does not depend on its construction family.

## `reference_point_long_solved_cache`

Spec §2 · layout: byte offsets · size: 293 B

Offsets begin at the byte after the UTF-16LE feature name. Unlisted bytes belong to the native construction state.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `object_header` | `bytes[8]` | little | spec | At name-end +0, the object header is `00 00 00 00 00 00 00 c0` |
| 8 | 4 | `object_id` | `u32` | little | spec | name-end +8 stores the feature object ID as u32 LE |
| 12 | 4 | `zero_after_id` | `bytes[4]` | little | spec | name-end +12 stores four zero bytes |
| 243 | 16 | `zero_before_position` | `bytes[16]` | little | spec | At name-end +227 or +243, sixteen zero bytes precede the position |
| 259 | 24 | `position` | `f64[3]` | little | spec | At name-end +243 or +259, three finite f64 LE values store xyz in metres |
| 283 | 2 | `construction_form` | `u16` | little | spec | At name-end +267 or +283, a u16 LE construction form is `4` or `5` |
| 285 | 8 | `zero_trailer` | `bytes[8]` | little | spec | At name-end +269 or +285, eight zero bytes terminate the solved-position cache |

Unstated regions:

- `16..243` (227 B): Native reference-point construction state; the solved datum position does not depend on its construction family.

## `extrusion_sparse_operation_trailer`

Spec §2 · layout: byte offsets · size: 40 B

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/operations.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `zero_header` | `u32` | little | spec | followed by four zero bytes |
| 4 | 2 | `family` | `u16` | little | spec | The family word establishes the operation family independently of a shared class token |
| 6 | 1 | `operation` | `u8` | little | spec | a one-byte Boolean operation |
| 7 | 1 | `schema` | `u8` | little | spec | one schema byte |
| 8 | 4 | `object_id` | `u32` | little | spec | the repeated little-endian u32 object identifier |
| 12 | 4 | `zero_after_object` | `u32` | little | spec | and four zero bytes |
| 16 | 6 | `sparse_zero_prefix` | `bytes[6]` | little | spec | stores six zero bytes at trailer +16 |
| 22 | 2 | `sparse_marker` | `u16` | little | spec | u16 `1` at +22 |
| 24 | 2 | `first_token` | `u16` | little | spec | a nonzero u16 token at +24 |
| 26 | 4 | `optional_identity` | `u32` | little | spec | an optional u32 identity at +26 |
| 30 | 8 | `zero_before_final_token` | `bytes[8]` | little | spec | eight zero bytes at +30 |
| 38 | 2 | `final_token` | `u16` | little | spec | a second nonzero u16 token at +38 |

## `coordinate_system_component_point`

Spec §2 · layout: byte offsets · size: 151 B

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/reference_geometry.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 10 | `prefix` | `bytes[10]` | little | spec | begins with family byte `2d` or `2f` and suffix `80 02 00 00 00 00 00 00 00` |
| 10 | 35 | `zero_header` | `bytes[35]` | little | spec | followed by 35 zero bytes |
| 45 | 16 | `sentinel` | `bytes[16]` | little | spec | sixteen `ff` bytes |
| 61 | 8 | `zero_before_source` | `bytes[8]` | little | spec | and eight zero bytes |
| 69 | 4 | `source_id` | `u32` | little | spec | Record +69 stores a nonzero u32 LE source ID |
| 73 | 4 | `source_stamp` | `u32` | little | spec | +73 stores a nonzero non-sentinel u32 LE source stamp |
| 77 | 2 | `zero_selector` | `u16` | little | spec | +77 stores u16 zero |
| 79 | 2 | `one_selector` | `u16` | little | spec | +79 stores u16 `1` |
| 81 | 6 | `zero_before_object` | `bytes[6]` | little | spec | +81 stores six zero bytes |
| 87 | 4 | `object_id` | `u32` | little | spec | +87 stores a nonzero u32 LE object ID |
| 91 | 12 | `zero_before_handles` | `bytes[12]` | little | spec | +91 stores twelve zero bytes |
| 103 | 8 | `handles` | `bytes[8]` | little | spec | Record +103 stores `c7 cf ff ff c7 cf ff ff` |
| 111 | 4 | `zero_before_generation` | `u32` | little | spec | +111 stores u32 zero |
| 115 | 4 | `generation` | `u32` | little | spec | +115 stores a nonzero non-sentinel u32 LE generation word |
| 119 | 8 | `zero_before_origin` | `bytes[8]` | little | spec | +119 stores eight zero bytes |
| 127 | 24 | `origin` | `f64[3]` | little | spec | Three finite f64 LE values at +127, +135, and +143 store the solved origin in metres |

## `coordinate_system_extended_component_point`

Spec §2 · layout: byte offsets · size: 165 B

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/reference_geometry.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 77 | `prefix_and_source` | `bytes[77]` | little | spec | has the same prefix through the source stamp at +73 |
| 77 | 4 | `reference_id` | `u32` | little | spec | Record +77 stores a nonzero non-sentinel u32 reference ID |
| 81 | 4 | `sentinel` | `u32` | little | spec | +81 stores `ff ff ff ff` |
| 85 | 4 | `zero_before_count` | `u32` | little | spec | +85 stores u32 zero |
| 89 | 4 | `reference_count` | `u32` | little | spec | +89 stores a positive non-sentinel u32 reference count |
| 93 | 4 | `one` | `u32` | little | spec | +93 stores u32 `1` |
| 97 | 4 | `zero_before_object` | `u32` | little | spec | +97 stores u32 zero |
| 101 | 4 | `object_id` | `u32` | little | spec | +101 stores a nonzero u32 object ID |
| 105 | 12 | `zero_before_handles` | `bytes[12]` | little | spec | +105 stores twelve zero bytes |
| 117 | 8 | `handles` | `bytes[8]` | little | spec | Record +117 stores `c7 cf ff ff c7 cf ff ff` |
| 125 | 4 | `zero_before_generation` | `u32` | little | spec | +125 stores u32 zero |
| 129 | 4 | `generation` | `u32` | little | spec | +129 stores the shared nonzero non-sentinel generation word |
| 133 | 8 | `zero_before_origin` | `bytes[8]` | little | spec | +133 stores eight zero bytes |
| 141 | 24 | `origin` | `f64[3]` | little | spec | Three finite f64 LE values at +141, +149, and +157 store the solved origin in metres |

## `coordinate_system_component_path_prefix`

Spec §2 · layout: byte offsets · size: 110 B

The counted compact component path starts immediately after this prefix. Its byte length depends on its typed entries and separators.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 73 | `common_prefix_and_source` | `bytes[73]` | little | spec | has the same 69-byte prefix and nonzero source ID as the fixed component-point records |
| 73 | 7 | `sentinel` | `bytes[7]` | little | spec | Record +73 stores seven `ff` bytes |
| 80 | 4 | `path_entry_count` | `u32` | little | spec | Record +80 stores a positive u32 LE path-entry count |
| 84 | 4 | `path_kind` | `bytes[4]` | little | spec | +84 stores a component-vector selector whose byte 1 is `02`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero |
| 88 | 4 | `zero_before_marker` | `u32` | little | spec | +88 stores four zero bytes |
| 92 | 16 | `component_marker` | `bytes[16]` | little | spec | +92 stores the duplicated compact component marker |
| 108 | 2 | `zero_before_path` | `u16` | little | spec | +108 stores two zero bytes |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/resolved_features/selections.rs` — The owning component-path parser starts counted entries 18 bytes after the compact marker.

## `coordinate_system_component_path_suffix`

Spec §2 · layout: byte offsets · size: 86 B

Path-end-relative. An optional eight-byte terminal null slot precedes this suffix and is not part of its size.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/reference_geometry.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 14 | `zero_header` | `bytes[14]` | little | spec | fourteen zero bytes |
| 14 | 4 | `one` | `u32` | little | spec | u32 LE `1` |
| 18 | 4 | `zero_before_object` | `u32` | little | spec | four zero bytes |
| 22 | 4 | `object_id` | `u32` | little | spec | a nonzero non-sentinel u32 LE object ID |
| 26 | 12 | `zero_before_handles` | `bytes[12]` | little | spec | and twelve zero bytes precede the solved suffix |
| 38 | 8 | `handles` | `bytes[8]` | little | spec | stores `c7 cf ff ff c7 cf ff ff` |
| 46 | 4 | `zero_before_generation` | `u32` | little | spec | u32 zero |
| 50 | 4 | `generation` | `u32` | little | spec | the shared nonzero non-sentinel generation word |
| 54 | 8 | `zero_before_origin` | `bytes[8]` | little | spec | eight zero bytes |
| 62 | 24 | `origin` | `f64[3]` | little | spec | three finite f64 LE origin coordinates in metres |

## `coordinate_system_ordinal_axis_tail`

Spec §2 · layout: byte offsets · size: 35 B

Origin-end-relative. One or two nonzero u16 tokens follow this fixed core and terminate the feature object.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/reference_geometry.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `x_axis_ordinal` | `u16` | little | spec | Two distinct u16 LE values at tail +0 and +2 are in the range `1..3` |
| 2 | 2 | `y_axis_ordinal` | `u16` | little | spec | Two distinct u16 LE values at tail +0 and +2 are in the range `1..3` |
| 4 | 23 | `zero_before_origin_z` | `bytes[23]` | little | spec | Tail +4 stores 23 zero bytes |
| 27 | 8 | `origin_z` | `f64` | little | spec | The f64 LE value at tail +27 repeats the component-point origin Z coordinate in metres |

## `coordinate_system_two_point_separator`

Spec §2 · layout: byte offsets · size: 14 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 6 | `selectors` | `u16[3]` | little | spec | stores u16 LE values `2`, `1`, and `0` |
| 6 | 2 | `first_token` | `u16` | little | spec | a nonzero u16 token |
| 8 | 2 | `one` | `u16` | little | spec | u16 `1` |
| 10 | 4 | `final_tokens` | `u16[2]` | little | spec | and two nonzero u16 tokens |

## `coordinate_system_two_point_tail`

Spec §2 · layout: byte offsets · size: 94 B

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/reference_geometry.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `origin_yz` | `f64[2]` | little | spec | Tail +0 and +8 repeat the first point's Y and Z coordinates in metres |
| 16 | 24 | `x_direction` | `f64[3]` | little | spec | A unit X direction occupies tail +16 through +39 |
| 40 | 1 | `separator` | `u8` | little | spec | Byte +40 is zero |
| 41 | 24 | `repeated_x_direction` | `f64[3]` | little | spec | the same direction is repeated at unaligned f64 offsets +41, +49, and +57 |
| 65 | 3 | `zero_before_origin` | `bytes[3]` | little | spec | Tail +65 stores three zero bytes |
| 68 | 24 | `origin` | `f64[3]` | little | spec | Three f64 LE values at +68 store the complete first point |
| 92 | 2 | `terminal_token` | `u16` | little | spec | tail +92 stores a nonzero u16 token |

## `coordinate_system_endpoint_path_prefix`

Spec §2 · layout: byte offsets · size: 110 B

The counted compact component path starts immediately after this fixed prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 17 | `family` | `bytes[17]` | little | spec | begins with `2f 80 02 00 00 00 40 00 00 75 00 00 00 75 00 00 00` |
| 17 | 28 | `zero_header` | `bytes[28]` | little | spec | followed by 28 zero bytes |
| 45 | 16 | `sentinel` | `bytes[16]` | little | spec | sixteen `ff` bytes |
| 61 | 8 | `zero_before_selector` | `bytes[8]` | little | spec | and eight zero bytes |
| 69 | 4 | `selector` | `u32` | little | spec | Record +69 stores a nonzero non-sentinel u32 LE selector |
| 73 | 7 | `zero_before_count` | `bytes[7]` | little | spec | +73 stores seven zero bytes |
| 80 | 4 | `path_entry_count` | `u32` | little | spec | +80 stores a positive u32 LE path-entry count |
| 84 | 4 | `path_kind` | `bytes[4]` | little | spec | +84 stores a component-vector selector whose byte 1 is `02`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero |
| 88 | 4 | `token` | `u32` | little | spec | +88 stores a nonzero non-sentinel u32 LE token |
| 92 | 16 | `component_marker` | `bytes[16]` | little | spec | +92 stores the duplicated compact component marker |
| 108 | 2 | `zero_before_path` | `u16` | little | spec | +108 stores two zero bytes |

## `coordinate_system_endpoint_path_suffix`

Spec §2 · layout: byte offsets · size: 142 B

Path-end-relative after the required eight-byte null slot.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/reference_geometry.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 70 | `zero_header` | `bytes[70]` | little | spec | 70 zero bytes |
| 70 | 4 | `one` | `u32` | little | spec | u32 LE `1` |
| 74 | 4 | `zero_before_object` | `u32` | little | spec | four zero bytes |
| 78 | 4 | `object_id` | `u32` | little | spec | a nonzero non-sentinel u32 LE object ID |
| 82 | 12 | `zero_before_handles` | `bytes[12]` | little | spec | and twelve zero bytes precede the 48-byte solved suffix |
| 94 | 8 | `handles` | `bytes[8]` | little | spec | The solved suffix uses the component-path point handles |
| 102 | 4 | `zero_before_generation` | `u32` | little | spec | generation, zero padding |
| 106 | 4 | `generation` | `u32` | little | spec | generation, zero padding |
| 110 | 8 | `zero_before_origin` | `bytes[8]` | little | spec | generation, zero padding |
| 118 | 24 | `origin` | `f64[3]` | little | spec | finite origin-coordinate layout |

## `coordinate_system_line_axis`

Spec §2 · layout: byte offsets · size: 113 B

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/reference_geometry.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `handles` | `bytes[8]` | little | spec | Record +0 stores `c7 cf ff ff c7 cf ff ff` |
| 8 | 4 | `zero_before_generation` | `u32` | little | spec | +8 stores u32 zero |
| 12 | 4 | `generation` | `u32` | little | spec | +12 stores the same generation word as the component-point record |
| 16 | 16 | `zero_before_scalar` | `bytes[16]` | little | spec | +16 stores sixteen zero bytes |
| 32 | 8 | `carrier_scalar` | `f64` | little | spec | Record +32 stores a positive finite f64 carrier scalar |
| 40 | 24 | `line_point` | `f64[3]` | little | spec | Three finite f64 LE values at +40 store a point on the selected line |
| 64 | 24 | `direction` | `f64[3]` | little | spec | Three f64 LE values at +64 store a unit direction |
| 88 | 1 | `separator` | `u8` | little | spec | Byte +88 is zero |
| 89 | 24 | `repeated_direction` | `f64[3]` | little | spec | The same direction is repeated as three f64 LE values at +89 |

## `coordinate_system_xy_tail`

Spec §2 · layout: byte offsets · size: 29 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `x_reversed` | `u8` | little | spec | Bytes +0, +1, and +2 are Boolean X, Y, and Z direction-reversal flags |
| 1 | 1 | `y_reversed` | `u8` | little | spec | Bytes +0, +1, and +2 are Boolean X, Y, and Z direction-reversal flags |
| 2 | 1 | `z_reversed` | `u8` | little | spec | The complete X/Y forms have a zero Z flag |
| 3 | 24 | `origin` | `f64[3]` | little | spec | Three finite f64 LE values at +3 store the origin in metres |
| 27 | 2 | `terminator` | `u16` | little | spec | The final u16 token is nonzero |

## `constructed_reference_plane_fixed_frame`

Spec §2 · layout: byte offsets · size: 97 B

Offsets begin immediately after the data-class name. The pairwise-orthogonal form uses both basis triples; the `moFixedRefPlnData_c` repeated-normal form uses one in-plane triple and duplicates the normal in the other. A valid 121-byte matrix frame at the same offset owns this 97-byte prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 24 | `origin` | `f64[3]` | little | spec | Three f64 values at offsets `+0`, `+8`, and `+16` store xyz origin coordinates in metres |
| 24 | 24 | `normal` | `f64[3]` | little | spec | Three f64 values at `+24`, `+32`, and `+40` store the unit normal |
| 48 | 1 | `frame_marker` | `u8` | little | spec | Byte `+48` is `1` in the 97-byte frame |
| 49 | 24 | `u_axis` | `f64[3]` | little | spec | In the pairwise-orthogonal form, unit in-plane u- and v-axes occupy the unaligned f64 triples at `+49`, `+57`, `+65` and `+73`, `+81`, `+89` |
| 73 | 24 | `v_axis` | `f64[3]` | little | spec | In the pairwise-orthogonal form, unit in-plane u- and v-axes occupy the unaligned f64 triples at `+49`, `+57`, `+65` and `+73`, `+81`, `+89` |

## `constructed_reference_plane_matrix_frame`

Spec §2 · layout: byte offsets · size: 121 B

Offsets begin immediately after the `moConstraintCoincLineAtAnglePlaneRefplaneData_c` class name.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 24 | `origin` | `f64[3]` | little | spec | Its origin, normal, and byte `1` use offsets `+0` through `+48` of the 97-byte frame |
| 24 | 24 | `normal` | `f64[3]` | little | spec | Its origin, normal, and byte `1` use offsets `+0` through `+48` of the 97-byte frame |
| 48 | 1 | `frame_marker` | `u8` | little | spec | Its origin, normal, and byte `1` use offsets `+0` through `+48` of the 97-byte frame |
| 49 | 72 | `basis_matrix` | `f64[9]` | little | spec | A right-handed orthonormal 3×3 matrix occupies the unaligned f64 fields at offsets `+49` through `+113` in row-major order |

## `component_face_nested_reference_prefix`

Spec §2 · layout: byte offsets · size: 102 B

Offsets begin at the `moCompFace_c` body. The nested class declaration is variable within the fixed region; the component-path entries follow the marker tail.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/selections.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `class_token` | `u16` | little | spec | class token at `+0` |
| 2 | 4 | `record_version` | `u32` | little | spec | u32 `2` at `+2` |
| 6 | 2 | `flags` | `bytes[2]` | little | spec | zero flags at `+6` |
| 84 | 16 | `component_marker` | `bytes[16]` | little | spec | component marker at `+84` |
| 100 | 2 | `marker_tail` | `u16` | little | spec | zero marker tail at `+100..+101` |

Unstated regions:

- `8..84` (76 B): The nested `moFaceRef_c` class declaration occupies a variable position before the component marker.

## `component_face_compact_reference_prefix`

Spec §2 · layout: byte offsets · size: 82 B

Offsets begin at the `moCompFace_c` body. The component-path entries follow the marker tail.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/selections.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `class_token` | `u16` | little | spec | class token at `+0` |
| 2 | 4 | `record_version` | `u32` | little | spec | u32 `2` at `+2` |
| 6 | 2 | `flags` | `bytes[2]` | little | spec | fixed zero-flag prefix |
| 64 | 16 | `component_marker` | `bytes[16]` | little | spec | component-path marker 64 bytes after the body start |
| 80 | 2 | `marker_tail` | `u16` | little | spec | zero marker tail at `+80..+81` |

Unstated regions:

- `8..64` (56 B): Fixed carrier bytes before the component marker.

## `temporary_axis_reference_nine_scalar`

Spec §2 · layout: byte offsets · size: 316 B

Offsets begin at the class declaration. The carrier body ends at +311; a following class marker at +312 terminates the record after zero padding of at most 24 bytes.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/axes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `class_marker` | `bytes[4]` | little | spec | The declaration starts with `ff ff 01 00` · value `[255, 255, 1, 0]` |
| 4 | 2 | `name_length` | `u16` | little | spec | name length `15` at `+4` · value `15` |
| 6 | 15 | `name` | `bytes[15]` | little | spec | class name `moTempAxisRef_w` at `+6` · value `[109, 111, 84, 101, 109, 112, 65, 120, 105, 115, 82, 101, 102, 95, 119]` |
| 223 | 8 | `handles` | `bytes[8]` | little | spec | two `c7 cf ff ff` handle words at declaration offsets `+223` and `+227` · value `[199, 207, 255, 255, 199, 207, 255, 255]` |
| 231 | 4 | `zero_before_address` | `bytes[4]` | little | spec | followed by a zero u32 and a nonzero stream address · value `[0, 0, 0, 0]` |
| 235 | 4 | `stream_address` | `u32` | little | spec | followed by a zero u32 and a nonzero stream address |
| 239 | 72 | `axis_frame` | `f64[9]` | little | spec | Nine little-endian f64 values at declaration offset `+239` store the axis point in metres in the first xyz triple and the unit axis direction in the final xyz triple. |
| 312 | 4 | `next_class_marker` | `bytes[4]` | little | spec | the next class declaration starts at `+312` · value `[255, 255, 1, 0]` |

Unstated regions:

- `21..223` (202 B): Undecoded class-body bytes between the class name and the handle pair.
- `311..312` (1 B): One byte separates the final f64 scalar from the next class marker.

## `cosmetic_thread_component_edge_wrapper_prefix`

Spec §2 · layout: byte offsets · size: 17 B

Offsets begin at the component-edge body. The compact edge-selection vector or the immediate edge-reference child follows this fixed wrapper prefix.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/selections.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `inner_class_token` | `u16` | little | spec | inner high-bit u16 class token at body +0 |
| 2 | 7 | `wrapper_flags` | `bytes[7]` | little | spec | byte `02` at +2, zero bytes at +3..+8 · value `[2, 0, 0, 0, 0, 0, 0]` |
| 9 | 4 | `component_count` | `u32` | little | spec | equal nonzero little-endian u32 component counts at +9 and +13 |
| 13 | 4 | `component_count_copy` | `u32` | little | spec | equal nonzero little-endian u32 component counts at +9 and +13 |

## `cosmetic_thread_repeated_edge_ref_prefix`

Spec §2 · layout: byte offsets · size: 8 B

Offsets begin at the body opened by the repeated edge-reference class token.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/selections.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `prefix` | `bytes[8]` | little | spec | `01 00 00 00 00 00 00 00` · value `[1, 0, 0, 0, 0, 0, 0, 0]` |

## `display_lists_scene_source_binding`

Spec §8 · layout: byte offsets · size: 16 B

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/tessellation.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 12 | `marker` | `bytes[12]` | little | spec | `00 00 00 00 00 00 30 40 00 00 00 00` · value `[0, 0, 0, 0, 0, 0, 48, 64, 0, 0, 0, 0]` |
| 12 | 4 | `source_id` | `u32` | little | spec | nonzero u32 LE source identifier |

## `display_lists_inline_visual_properties_prefix`

Spec §8 · layout: byte offsets · size: 22 B

The variable-length UTF-16LE material name begins at the end of this prefix.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/appearance.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `marker` | `bytes[2]` | little | spec | begins with `33 80` · value `[51, 128]` |
| 2 | 4 | `packed_color` | `u32` | little | spec | packed `0x00BBGGRR` colour is u32 LE at `+2` |
| 6 | 12 | `uninterpreted` | `bytes[12]` | little | derived | The bytes are retained without assigned semantics. |
| 18 | 3 | `name_marker` | `bytes[3]` | little | spec | Bytes `ff fe ff` at `+18` · value `[255, 254, 255]` |
| 21 | 1 | `name_length` | `u8` | little | spec | u8 UTF-16 code-unit count at `+21` |

## `visual_states_feature_appearance_prefix`

Spec §8 · layout: byte offsets · size: 36 B

The prefix ends after the packed colour. The remaining visual-property payload is outside this fixed layout.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/appearance.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `version` | `u32` | little | spec | begins with u32 LE version `17000` · value `17000` |
| 4 | 4 | `feature_source_id` | `u32` | little | spec | the feature source ID |
| 8 | 4 | `feature_timestamp` | `u32` | little | spec | the feature timestamp |
| 12 | 4 | `selector_one_a` | `u32` | little | spec | u32 LE values `1`, `1`, and `2` · value `1` |
| 16 | 4 | `selector_one_b` | `u32` | little | spec | u32 LE values `1`, `1`, and `2` · value `1` |
| 20 | 4 | `selector_two` | `u32` | little | spec | u32 LE values `1`, `1`, and `2` · value `2` |
| 24 | 6 | `instance_prefix` | `bytes[6]` | little | spec | bytes `07 80 01 00 00 00` · value `[7, 128, 1, 0, 0, 0]` |
| 30 | 2 | `marker` | `bytes[2]` | little | spec | marker `09 80` · value `[9, 128]` |
| 32 | 4 | `packed_color` | `u32` | little | spec | packed `0x00BBGGRR` colour follows the marker |

## `transformed_reference_plane_metadata`

Spec §8 · layout: byte offsets · size: 80 B

Offsets begin immediately after the `moTransRefPlaneData_c` class token.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/metadata.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `prefix` | `bytes[8]` | little | spec | Fixed prefix. · value `[255, 255, 255, 255, 255, 255, 255, 255]` |
| 8 | 24 | `center` | `f64[3]` | little | spec | Plane center xyz in metres. |
| 32 | 16 | `extents` | `f64[2]` | little | spec | Plane extents in metres. |
| 48 | 24 | `auxiliary_frame` | `f64[3]` | little | spec | Dimensionless auxiliary frame. |
| 72 | 8 | `diagonal` | `f64` | little | spec | Plane diagonal in metres. |

## `display_lists_compact_face_header`

Spec §8 · layout: byte offsets · size: 8 B

Offsets begin after the `uoTempFaceTessData_c` class token. The first descriptor starts at the end of this compact header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `triangle_count` | `u32` | little | spec | triangle count as u32 LE at `+0` |
| 4 | 4 | `strip_count` | `u32` | little | spec | strip count as u32 LE at `+4` |

## `display_lists_extended_face_header`

Spec §8 · layout: byte offsets · size: 40 B

Offsets begin after the `uoTempFaceTessData_c` class token. The first descriptor starts at the end of this extended header.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/tessellation.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `triangle_count` | `u32` | little | spec | triangle count as u32 LE at `+0` |
| 4 | 4 | `strip_count` | `u32` | little | spec | strip count as u32 LE at `+4` |
| 8 | 4 | `form` | `u32` | little | spec | u32 LE values `1`, `0`, `0`, and one nonzero token at `+8`, `+12`, `+16`, and `+20` |
| 12 | 4 | `zero_at_12` | `u32` | little | spec | u32 LE values `1`, `0`, `0`, and one nonzero token at `+8`, `+12`, `+16`, and `+20` |
| 16 | 4 | `zero_at_16` | `u32` | little | spec | u32 LE values `1`, `0`, `0`, and one nonzero token at `+8`, `+12`, `+16`, and `+20` |
| 20 | 4 | `form_token` | `u32` | little | spec | u32 LE values `1`, `0`, `0`, and one nonzero token at `+8`, `+12`, `+16`, and `+20` |
| 24 | 16 | `zero_tail` | `bytes[16]` | little | spec | followed by 16 zero bytes at `+24` |

## `draft_plane_reference_prefix`

Spec §2 · layout: byte offsets · size: 112 B

The variable component-path entries follow this prefix. Offsets begin at the lane-scoped plane-reference token.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `token` | `u16` | little | spec | +0 \| u16 LE \| lane-scoped plane-reference token |
| 2 | 2 | `child_token` | `u16` | little | spec | +2 \| u16 LE \| non-`ffff` tagged child token |
| 4 | 4 | `form` | `u32` | little | spec | +4 \| u32 LE \| value `2` |
| 8 | 3 | `wrapper_flags` | `bytes[3]` | little | spec | +8 \| bytes[3] \| `00 00 00` or `40 00 00` wrapper flags |
| 11 | 4 | `identity` | `u32` | little | spec | +11 \| u32 LE \| nonzero reference identity |
| 15 | 4 | `identity_copy` | `u32` | little | spec | +15 \| u32 LE \| repeated reference identity |
| 47 | 16 | `sentinel` | `bytes[16]` | little | spec | +47 \| bytes[16] \| `ff` |
| 72 | 2 | `instance_token` | `u16` | little | spec | +72 \| u16 LE \| tagged instance token |
| 74 | 4 | `role` | `u32` | little | spec | +74 \| u32 LE \| role word |
| 78 | 4 | `zero_at_78` | `u32` | little | spec | +78 \| u32 LE \| zero |
| 82 | 4 | `cell_count` | `u32` | little | spec | +82 \| u32 LE \| component-vector cell count in `2..65` |
| 86 | 4 | `path_kind` | `bytes[4]` | little | spec | +86 \| bytes[4] \| component-vector selector: byte 1 is `02` or `03`, byte 0 is a lane-specific subtype, and bytes 2–3 are zero |
| 90 | 4 | `selector` | `u32` | little | spec | +90 \| u32 LE \| component selector |
| 94 | 16 | `component_marker` | `bytes[16]` | little | spec | +94 \| bytes[16] \| duplicated component-vector marker |
| 110 | 2 | `marker_tail` | `u16` | little | spec | +110 \| u16 LE \| zero marker tail |

Unstated regions:

- `19..47` (28 B): Zero bytes.
- `63..72` (9 B): Zero bytes.

## `draft_compact_selection_prefix`

Spec §2 · layout: byte offsets · size: 30 B

Variable mixed component paths follow this prefix. Offsets begin at the bounded cell field.

Parsed by:
- `crates/cadmpeg-codec-sldprt/src/resolved_features/drafts.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `cell_field` | `u32` | little | spec | a bounded u32 LE cell field in `1..65` |
| 4 | 4 | `selection_role` | `bytes[4]` | little | spec | a component-vector selector whose byte 1 is `02` for the parting-tool selection or `03` for a drafted-face selection, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero |
| 8 | 4 | `selector` | `u32` | little | spec | a u32 LE selector |
| 12 | 16 | `component_marker` | `bytes[16]` | little | spec | the 16-byte duplicated component marker |
| 28 | 2 | `marker_tail` | `u16` | little | spec | a u16 LE zero tail |

## `draft_aligned_direction_frame`

Spec §2 · layout: byte offsets · size: 120 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `handles` | `bytes[8]` | little | spec | The frame starts with `c7 cf ff ff c7 cf ff ff` |
| 8 | 4 | `zero_at_8` | `u32` | little | spec | u32 zero |
| 12 | 4 | `address` | `u32` | little | spec | a nonzero u32 address |
| 96 | 24 | `pull_direction` | `f64[3]` | little | spec | the xyz pull-direction unit vector is the final three values at +96, +104, and +112 |

Unstated regions:

- `16..24` (8 B): Zero bytes.
- `24..96` (72 B): Nine finite f64 LE values precede the pull direction.

## `draft_extended_direction_frame`

Spec §2 · layout: byte offsets · size: 153 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `handles` | `bytes[8]` | little | spec | The frame starts with `c7 cf ff ff c7 cf ff ff` |
| 8 | 4 | `zero_at_8` | `u32` | little | spec | u32 zero |
| 12 | 4 | `address` | `u32` | little | spec | a nonzero u32 address |
| 129 | 24 | `pull_direction` | `f64[3]` | little | spec | an unaligned xyz unit vector occupies +129, +137, and +145 |

Unstated regions:

- `16..24` (8 B): Zero bytes.
- `24..120` (96 B): Twelve finite f64 LE values; the final three do not form a unit vector in this form.
- `120..129` (9 B): Zero-byte extended-form discriminator.

## `compact_current_spatial_marker_point`

Spec §2 · layout: byte offsets · size: 82 B

The fixed point prefix ends after the third coordinate; any marker-specific trailer follows outside this prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | A current-prefix compact profile-locus spatial point |
| 5 | 8 | `header` | `bytes[8]` | little | spec | eight `ff` bytes |
| 13 | 4 | `sentinel` | `f32` | little | spec | little-endian f32 `-1.0` |
| 17 | 4 | `native_kind` | `u32` | little | spec | native kind u32 `0` or `1` |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | profile locus `04 00 02 00` |
| 27 | 2 | `profile_role` | `u16` | little | spec | profile role u16 `1` |
| 31 | 8 | `selector` | `bytes[8]` | little | spec | selector `00 00 80 bf 00 00 04 00` at marker +31 |
| 48 | 8 | `state_value` | `f64` | little | spec | f64 `1` at marker +48 |
| 56 | 2 | `coordinate_tag` | `bytes[2]` | little | spec | coordinate tag `0e 00` at marker +56 |
| 58 | 24 | `coordinates` | `f64[3]` | little | spec | xyz coordinates at marker +58 |

Unstated regions:

- `21..23` (2 B): Reserved bytes before the profile locus.
- `29..31` (2 B): The state prefix is reserved in this compact point form.
- `39..48` (9 B): The state value begins at +48; bytes +39 through +47 are reserved.

## `wide_spatial_marker_coordinate_prefix`

Spec §2 · layout: byte offsets · size: 90 B

The fixed coordinate prefix is shared by point and relation-handle markers. The record trailer follows the third coordinate.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `marker` | `bytes[5]` | little | spec | An object-indexed marker-backed spatial point begins with `ff ff 07 00 01`, `ff ff 1f 00 01`, or `ff ff 1f 00 03` |
| 5 | 8 | `header` | `bytes[8]` | little | spec | eight `ff` bytes |
| 13 | 4 | `sentinel` | `f32` | little | spec | little-endian f32 `-1.0` |
| 17 | 4 | `native_kind` | `u32` | little | spec | the native kind is at marker +17 |
| 23 | 4 | `profile_locus` | `bytes[4]` | little | spec | role bytes `04 00 02 00` |
| 27 | 2 | `profile_role` | `u16` | little | spec | profile role u16 `1` |
| 48 | 8 | `state_value` | `f64` | little | spec | marker +48 stores f64 `1` |
| 64 | 2 | `coordinate_tag` | `bytes[2]` | little | spec | marker +64 contains `0e 00` |
| 66 | 24 | `coordinates` | `f64[3]` | little | spec | the coordinates begin at marker +66 |

Unstated regions:

- `21..23` (2 B): Reserved bytes before the profile locus.
- `29..48` (19 B): Marker state, selector, and reserved bytes precede the state value.
- `56..64` (8 B): Zero bytes before the coordinate tag.

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| ResolvedFeatures sketch and feature-input markers (§2) | §2 | The remaining marker layouts are about 125 distinct records, each one prose paragraph stating marker-relative offsets for a specific record length. The fixed-offset layouts above cover the currently tabulated profile, sketch-input, and reference-plane forms; the remaining paragraphs are transcribable in principle and can be added incrementally. |
| Body records (§6) | §6 | Apart from the class-root directory, §6 states slot-reference graphs and population invariants over about thirty named disc layouts. Those layouts state no byte offsets; their slot values are reached through the §5 common header and the §10 framing arithmetic. |
| Inline record framing (§10) | §10 | Framing arithmetic rather than a fixed-offset record: the zero byte after a prefixed triple run self-delimits that form, while `end = pos + 14 + 2*slot_count` for a bare record. Specification section 5 gives the supported schema, disc, and flo slot-count table. |
| SWIFT semantic PMI object graph (§2.1) | §2.1 | Token-framed variable-length grammar. Every entity and section is delimited by Pascal-string tokens and counted key/value or relation rosters; no field has a fixed offset from the stream or entity start. |
| Compound File Binary directory entry (§1) | §1 | The spec states the 128-byte entry size and names the fields but states no offset for any of them; the layout is the external CFB specification, not a cadmpeg finding. |
