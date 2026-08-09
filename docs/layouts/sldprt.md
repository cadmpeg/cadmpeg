<!-- Generated from docs/layouts/sldprt.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `sldprt` record layouts

Source of truth: [`docs/formats/sldprt.md`](../../docs/formats/sldprt.md).
Table source: `docs/layouts/sldprt.toml`.

Covers the container envelopes (§1, §1.1-§1.3), the typed topology tag
inventory (§4), the entity common header (§5), and the Parasolid geometry
carriers (§7.1-§7.4). §2 documents about 125 distinct ResolvedFeatures marker
layouts in prose; the fixed-offset profile and sketch-input layouts are tabulated
below, and the remaining layouts are listed under "Not tabulated" with a coverage
note.

Endianness is stated per lane: §1 container words are little-endian, §4-§7
Parasolid payload words are big-endian. Where a §1 field states no endianness
the table says `unstated` and says so in the field note.

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `00 0e` | bridge | 37 B | face-use → surface link; magic at body +8; bare record length 37 | §4 |
| `00 0f` | loop head | variable | bare record length is at least 14; no magic | §4 |
| `00 10` | edge-use | 28 B | magic at body +8 | §4 |
| `00 11` | oriented coedge | 21 B | no magic | §4 |
| `00 12` | vertex-use | 24 B | magic at body +16 | §4 |
| `00 1d` | world point | 38 B | no magic; four references at body +6 and xyz as three f64 BE at body +14 | §4 |

## `outer_header`

Spec §1.1 · layout: byte offsets · size: 8 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `file_id` | `u32` | unstated | spec | The spec states the width but not the byte order of this field, unlike `version` on the same line. |
| 4 | 4 | `version` | `u32` | big | spec | `version` (u32 **big-endian**, value `0x00000004`) |

## `block_frame_header`

Spec §1.1 · layout: byte offsets · size: 26 B

Fixed prefix only. `preamble[pre_sz]` and `payload[comp_sz]` follow; the record extent is `block_end = marker_offset + 26 + pre_sz + comp_sz`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 6 | `marker` | `bytes[6]` | little | spec | marker bytes[6] ; 14 00 06 00 08 00 |
| 6 | 4 | `type_id` | `u32` | little | spec | type_id u32 LE |
| 10 | 4 | `crc32` | `u32` | little | spec | crc32 u32 LE ; CRC-32 of the DECOMPRESSED payload |
| 14 | 4 | `comp_sz` | `u32` | little | spec | comp_sz u32 LE |
| 18 | 4 | `uncomp_sz` | `u32` | little | spec | uncomp_sz u32 LE |
| 22 | 4 | `pre_sz` | `u32` | little | spec | pre_sz u32 LE |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/container.rs` — The parser's block-header length matches the 26-byte fixed prefix this table tiles.
- `crates/cadmpeg-codec-sldprt/src/container.rs` — The parser's marker matches the spec's stated marker bytes.

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

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `magic` | `bytes[16]` | little | spec | The wrapper is the 16-byte magic `23 1d d5 71 da 81 48 a2 a8 58 98 b2 1b 89 ef 99` |
| 16 | 4 | `uncompressed_size` | `u32` | little | spec | followed by the uncompressed byte count as u32 LE |
| 20 | 4 | `zlib_member_size` | `u32` | little | spec | the complete zlib-member byte count as u32 LE |

Cross-checked against code:

- `crates/cadmpeg-codec-sldprt/src/container.rs` — The parser names the same 16-byte wrapper magic.

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

Body-relative, after the two-byte family tag. An optional `ff` byte can occur between the `00 51` tag and `flags`; it shifts every following field by one byte. Slot values follow at +12 with a slot count keyed by `(schema, disc, flo)` that the spec does not enumerate.

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

## `compact_analytic_header`

Spec §7.1 · layout: byte offsets · size: 17 B

Body-relative, after the two-byte `00 TT` tag and the optional `ff`. `values f64 BE[n]` follows at +17; `n` is the per-tag f64 count. Total record size is `2 + [1] + 17 + 8n`.

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

## `bspline_array_header`

Spec §7.2 · layout: byte offsets · size: 6 B

Shared header of `00 2d` (poles, f64 elements), `00 7f` (knot multiplicities, u16 elements), and `00 80` (unique knot values, f64 elements). Offsets are relative to the byte after the tag and the marker. Element data follows at +6.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `count` | `u32` | big | spec | value_count u32 BE |
| 4 | 2 | `attr` | `u16` | big | spec | attr u16 BE |

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

Offsets are body-relative as the spec writes them. `count` point entries follow at +52; an entry is either 88 bytes (point xyz, then a unit tangent at entry +56) or a bare 24-byte point.

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
| 31 | 8 | `selector` | `bytes[8]` | little | spec | `00 00 80 bf 00 00 04 00` at marker +31 |
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

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| ResolvedFeatures sketch and feature-input markers (§2) | §2 | The remaining marker layouts are about 125 distinct records, each one prose paragraph stating marker-relative offsets for a specific record length. The fixed-offset layouts above cover the currently tabulated profile and sketch-input forms; the remaining paragraphs are transcribable in principle and can be added incrementally. |
| Body records (§6) | §6 | §6 states slot-reference graphs and population invariants over about thirty named disc layouts. It states no byte offsets; the slot values are reached through the §5 common header and the §10 framing arithmetic. |
| Inline record framing (§10) | §10 | Framing arithmetic rather than a record: `end = pos + 14 + 3*slot_count + 1` for a prefixed subrecord and `end = pos + 14 + 2*slot_count` for a bare one. The slot-count table it depends on is an open item, not a stated layout. |
| Compound File Binary directory entry (§1) | §1 | The spec states the 128-byte entry size and names the fields but states no offset for any of them; the layout is the external CFB specification, not a cadmpeg finding. |
| Tessellation and appearance lanes (§8) | §8 | §8 states descriptor relations and packing rules but no offsets. The parser asserts fixed offsets here that the spec does not state; recorded in the pull request. |
