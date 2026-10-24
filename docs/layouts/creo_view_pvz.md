<!-- Generated from docs/layouts/creo_view_pvz.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `creo_view_pvz` record layouts

Source of truth: [`docs/formats/creo_view_pvz.md`](../../docs/formats/creo_view_pvz.md).
Table source: `docs/layouts/creo_view_pvz.toml`.

The PVS member is a fixed header, a stream directory, and GZIP streams whose
expanded bytes use a variable-width element grammar (§3.4). Only the header and
the fixed head of a directory entry have fixed offsets. The OL member is a
fixed header, a directory of 8-byte entries, an optional 19-byte preamble
block, and GZIP streams of TLV records. The exact-geometry records use a scalar
width selected by the stream identity (§8.1); records whose layout depends on
that width are tabled once per width.

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `0x02` | DESC_U8_LENGTH | variable | Element descriptor: one-byte length, no type, no children. | §3.4 |
| `0x04` | DESC_U16_LENGTH | variable | Element descriptor: two-byte length, no type, no children. | §3.4 |
| `0x12` | DESC_U8_LENGTH_U8_TYPE | variable | Element descriptor: one-byte length, one-byte type, no children. | §3.4 |
| `0x22` | DESC_U8_LENGTH_U16_TYPE | variable | Element descriptor: one-byte length, two-byte type, no children. | §3.4 |
| `0x80` | DESC_CHILDREN | variable | Element descriptor: no length, no type, child list. | §3.4 |
| `0x82` | DESC_U8_LENGTH_CHILDREN | variable | Element descriptor: one-byte length, no type, child list. | §3.4 |
| `0x92` | DESC_U8_LENGTH_U8_TYPE_CHILDREN | variable | Element descriptor: one-byte length, one-byte type, child list. | §3.4 |
| `0x01000000` | STREAM_ATTRIBUTES | variable | OL stream identity: attributes and appearances. | §4.5 |
| `0x01000001` | STREAM_SURFACE_TESSELLATION | variable | OL stream identity: surface tessellation. | §4.5 |
| `0x01000002` | STREAM_EDGE_TESSELLATION | variable | OL stream identity: edge tessellation. The same value is the tag of the stream index record at the start of every stream. | §4.5 |
| `0x01000003` | STREAM_EXACT_F32 | variable | OL stream identity: exact geometry, version 3, f32 scalars. | §4.5 |
| `0x01000004` | STREAM_EXACT_F64 | variable | OL stream identity: exact geometry, version 4, f64 scalars. | §4.5 |
| `0x00ffffff` | STREAM_END_SENTINEL | variable | OL directory end sentinel; its offset equals the file length. | §4.5 |
| `0x05` | ATTR_APPEARANCE_VALUES | 56 B | Attribute stream: 14 f32 appearance values. | §5.1 |
| `0x07` | ATTR_APPEARANCE_REFERENCE | variable | Attribute stream: appearance table entry. | §5.2 |
| `0x0100000d` | TESS_PATCH_HEADER | variable | Surface tessellation: patch header. | §6.2 |
| `0x01000016` | TESS_VERTEX_BLOCK | variable | Tessellation: vertex block. | §6.3 |
| `0x19` | TESS_TRIANGLE_FAN | variable | Triangle fan of u32 indexes. | §6.4 |
| `0x1a` | TESS_TRIANGLE_STRIP | variable | Triangle strip of u32 indexes. | §6.4 |
| `0x1b` | TESS_TRIANGLE_LIST | variable | Triangle list of u32 indexes. | §6.4 |
| `0x0100000f` | EDGE_TESS_INDEX_TABLE | variable | Edge tessellation: u32 count and u32 values. | §7.1 |
| `0x1c` | TESS_LINE_STRIP | variable | Line strip of u32 indexes. | §7.2 |
| `0x1d` | TESS_LINE_SEGMENTS | variable | Line segments as u32 index pairs. | §7.2 |
| `0x13` | EXACT_SURFACE | variable | Exact stream: supporting surface; starts a face. | §8.2 |
| `0x01000010` | EXACT_LOOP | 4 B | Exact stream: loop record with an i32 marker. | §8.2 |
| `0x11` | EXACT_COEDGE | 13 B | Exact stream: coedge record. | §8.2 |
| `0x01000012` | EXACT_SINGULAR_USE | variable | Exact stream: singular edge use; 4 + 2w bytes. The same value is the tag of the edge header that opens an edge group (§7.2) or a line group (§6.1). | §8.2 |
| `0x14` | EXACT_EDGE_CURVE | variable | Exact stream: 3D edge curve. | §8.2 |
| `0x15` | EXACT_PCURVE | variable | Exact stream: 2D pcurve. | §8.2 |

## `pvs_header`

Spec §3.1 · layout: byte offsets · size: 18 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 10 | `signature` | `bytes[10]` | little | spec | \| 0 \| 10 \| `signature` \| |
| 10 | 4 | `first_stream_offset` | `u32` | little | spec | \| 10 \| 4 \| `first_stream_offset` \| |
| 14 | 4 | `stream_count` | `u32` | little | spec | \| 14 \| 4 \| `stream_count` \| |

## `pvs_stream_entry_head`

Spec §3.2 · layout: byte offsets · size: 3 B

Fixed head only. `name_length` name bytes follow at +3, then the `u32` stored length.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `stream_type` | `u8` | little | spec | \| 0 \| 1 \| `stream_type` \| |
| 1 | 1 | `codec` | `u8` | little | spec | \| 1 \| 1 \| `codec` \| Stream encoding. Value 1 is GZIP. \| · value `1` |
| 2 | 1 | `name_length` | `u8` | little | spec | \| 2 \| 1 \| `name_length` \| |

## `ol_header`

Spec §4.1 · layout: byte offsets · size: 22 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 10 | `signature` | `bytes[10]` | little | spec | \| 0 \| 10 \| `signature` \| |
| 10 | 4 | `version` | `u32` | little | spec | \| 10 \| 4 \| `version` \| `u32`. Value 1. \| · value `1` |
| 14 | 4 | `directory_length` | `u32` | little | spec | \| 14 \| 4 \| `directory_length` \| |
| 18 | 4 | `entry_count` | `u32` | little | spec | \| 18 \| 4 \| `entry_count` \| |

## `ol_directory_entry`

Spec §4.2 · layout: byte offsets · size: 8 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `stream_id` | `u32` | little | spec | \| 0 \| 4 \| `stream_id` \| |
| 4 | 4 | `offset` | `u32` | little | spec | \| 4 \| 4 \| `offset` \| |

## `ol_preamble_block`

Spec §4.3 · layout: byte offsets · size: 19 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `tag` | `u32` | little | spec | \| 0 \| 4 \| `tag` \| `u32` `0x010000d4`. \| · value `16777428` |
| 4 | 4 | `word` | `u32` | little | spec | \| 4 \| 4 \| `word` \| `u32` 2 or 3. \| |
| 8 | 1 | `byte_0` | `u8` | little | spec | \| 8 \| 1 \| `byte_0` \| `0x03`. \| · value `3` |
| 9 | 1 | `generation` | `u8` | little | spec | \| 9 \| 1 \| `generation` \| |
| 10 | 1 | `byte_2` | `u8` | little | spec | \| 10 \| 1 \| `byte_2` \| |
| 11 | 8 | `reserved` | `bytes[8]` | little | spec | \| 11 \| 8 \| `reserved` \| Zero bytes. \| · value `[0, 0, 0, 0, 0, 0, 0, 0]` |

## `tlv_header`

Spec §4.4 · layout: byte offsets · size: 8 B

`length` payload bytes follow at +8.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `tag` | `u32` | little | spec | \| 0 \| 4 \| `tag` (`u32`) \| |
| 4 | 4 | `length` | `u32` | little | spec | \| 4 \| 4 \| `length` (`u32`) \| |

## `appearance_values`

Spec §5.1 · layout: byte offsets · size: 56 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 12 | `base_rgb` | `f32[3]` | little | spec | \| 0–2 \| Base colour RGB. \| |
| 12 | 12 | `diffuse_rgb` | `f32[3]` | little | spec | \| 3–5 \| Diffuse RGB. \| |
| 24 | 12 | `specular_rgb` | `f32[3]` | little | spec | \| 6–8 \| Specular RGB. \| |
| 36 | 4 | `shininess` | `f32` | little | spec | \| 9 \| Shininess exponent. \| |
| 40 | 12 | `emissive_rgb` | `f32[3]` | little | spec | \| 10–12 \| Emissive RGB. \| |
| 52 | 4 | `opacity` | `f32` | little | spec | \| 13 \| Opacity, 1 for opaque. \| |

## `appearance_reference`

Spec §5.2 · layout: ordered slots (no stated byte offsets) · size: not stated

u32 name_length, name bytes, u32 base_index at 4 + name_length, then a 17-byte tail.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `name_length` | `u32` | little | spec | \| 0 \| 4 \| `name_length` (`u32`) \| |
| 1 | `name` | `cstring` | little | spec | `name`: NUL-terminated bytes, padded with NUL to `name_length`. |
| 2 | `base_index` | `u32` | little | spec | \| 4 + `name_length` \| 4 \| `base_index` (`u32`) \| |
| 3 | `tail` | `bytes[17]` | little | spec | \| 8 + `name_length` \| 17 \| `tail` \| |

## `patch_header_head`

Spec §6.2 · layout: byte offsets · size: 13 B

`ref_count` u32 values follow at +13; the payload length is 13 + 4 × ref_count.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `appearance` | `u32` | little | spec | \| 0 \| 4 \| `appearance` \| |
| 4 | 4 | `face_ref` | `u32` | little | spec | \| 4 \| 4 \| `face_ref` \| |
| 8 | 1 | `flag` | `u8` | little | spec | \| 8 \| 1 \| `flag` \| 0 or 1. \| |
| 9 | 4 | `ref_count` | `u32` | little | spec | \| 9 \| 4 \| `ref_count` \| |

## `vertex_block_head`

Spec §6.3 · layout: byte offsets · size: 5 B

vertex_count vertices follow at +5 with the stride the layout byte selects (12, 24, or 32).

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `layout` | `u8` | little | spec | \| 0 \| 1 \| `layout` \| |
| 1 | 4 | `vertex_count` | `u32` | little | spec | \| 1 \| 4 \| `vertex_count` (`u32`) \| |

## `edge_tessellation_header`

Spec §7.2 · layout: byte offsets · size: 12 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `edge_index` | `u32` | little | spec | \| 0 \| 4 \| `edge_index` \| |
| 4 | 4 | `word` | `u32` | little | spec | \| 4 \| 4 \| `word` \| `u32`. \| |
| 8 | 4 | `value` | `f32` | little | spec | \| 8 \| 4 \| `value` \| `f32`. \| |

## `surface_record_head`

Spec §8.3 · layout: byte offsets · size: 3 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `surface_type` | `u8` | little | spec | \| 0 \| 1 \| `surface_type` \| |
| 1 | 1 | `flags` | `u8` | little | spec | \| 1 \| 1 \| `flags` \| |
| 2 | 1 | `marker` | `u8` | little | spec | \| 2 \| 1 \| `marker` \| |

## `bspline_surface_head`

Spec §8.9 · layout: byte offsets · size: 12 B

u_knot_count scalar knots follow at +12.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `surface_type` | `u8` | little | spec | \| 0 \| 1 \| `surface_type` = 4 \| · value `4` |
| 1 | 1 | `flags` | `u8` | little | spec | \| 1 \| 1 \| `flags` \| |
| 2 | 1 | `marker` | `u8` | little | spec | \| 2 \| 1 \| `marker` \| |
| 3 | 1 | `rational` | `u8` | little | spec | \| 3 \| 1 \| `rational`: 0 or 1 \| |
| 4 | 4 | `u_order` | `u32` | little | derived | The body begins after the four head bytes. |
| 8 | 4 | `u_knot_count` | `u32` | little | derived | Follows the four-byte u_order. |

## `loop_record`

Spec §8.11 · layout: byte offsets · size: 4 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `marker` | `i32` | little | spec | has a 4-byte payload `marker` (`i32`) |

## `coedge_record`

Spec §8.12 · layout: byte offsets · size: 13 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `edge` | `u32` | little | spec | \| 0 \| 4 \| `edge` \| |
| 4 | 1 | `orientation` | `u8` | little | spec | \| 4 \| 1 \| `orientation` \| 0 or 1. \| |
| 5 | 4 | `pcurve` | `u32` | little | spec | \| 5 \| 4 \| `pcurve` \| |
| 9 | 4 | `pcurve_secondary` | `u32` | little | spec | \| 9 \| 4 \| `pcurve_secondary` \| |

## `singular_use_f32`

Spec §8.13 · layout: byte offsets · size: 12 B

Exact version 3 (w = 4).

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `edge` | `u32` | little | spec | \| 0 \| 4 \| `edge` (`u32`) \| |
| 4 | 4 | `t_start` | `f32` | little | derived | Scalar width w = 4 for exact version 3 (§8.1). |
| 8 | 4 | `t_end` | `f32` | little | derived | Offset 4 + w with w = 4. |

## `singular_use_f64`

Spec §8.13 · layout: byte offsets · size: 20 B

Exact version 4 (w = 8).

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `edge` | `u32` | little | spec | \| 0 \| 4 \| `edge` (`u32`) \| |
| 4 | 8 | `t_start` | `f64` | little | derived | Scalar width w = 8 for exact version 4 (§8.1). |
| 12 | 8 | `t_end` | `f64` | little | derived | Offset 4 + w with w = 8. |

## `bspline_curve_head`

Spec §8.15 · layout: byte offsets · size: 11 B

knot_count scalar knots follow at +11. The same head opens a B-spline pcurve (§8.16).

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `curve_type` | `u8` | little | spec | Every edge curve record begins with `curve_type` (`u8`) and `flags` (`u8`) · value `6` |
| 1 | 1 | `flags` | `u8` | little | spec | Every edge curve record begins with `curve_type` (`u8`) and `flags` (`u8`) |
| 2 | 1 | `rational` | `u8` | little | derived | First field after the two head bytes. |
| 3 | 4 | `order` | `u32` | little | derived | Follows the one-byte rational flag. |
| 7 | 4 | `knot_count` | `u32` | little | derived | Follows the four-byte order. |

## `polyline_head`

Spec §8.15 · layout: byte offsets · size: 6 B

count points of three scalars follow at +6, then one 0x00 byte. A polyline pcurve (§8.16) uses the same head with two scalars per point.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `curve_type` | `u8` | little | spec | Every edge curve record begins with `curve_type` (`u8`) and `flags` (`u8`) · value `7` |
| 1 | 1 | `flags` | `u8` | little | spec | Every edge curve record begins with `curve_type` (`u8`) and `flags` (`u8`) |
| 2 | 4 | `count` | `u32` | little | derived | First field after the two head bytes. |

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| PVS element streams | §3.4 | Variable-width elements: the descriptor selects the length and type widths, payloads hold ULEB128 atoms, and child lists nest. |
| Analytic surface, conic, and line records | §8.3 | Scalar width depends on the exact version, and range pairs depend on flag bits; the field order is a sequence of scalars. |
