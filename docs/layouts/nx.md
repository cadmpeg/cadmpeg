<!-- Generated from docs/layouts/nx.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `nx` record layouts

Source of truth: [`docs/formats/siemens_nx.md`](../../docs/formats/siemens_nx.md).
Table source: `docs/layouts/nx.toml`.

Covers the SPLMSSTR and legacy CFB containers (§2 and §2.4), the Parasolid XT fixed record families
(§4.1), the topology node field maps (§5.1), the analytic payload offsets
(§6.1), the B-spline descriptor prefixes (§6.2), the trimmed and SP curve
carriers (§6.4), the rolling-ball blend (§6.5), and the CHART_s preamble
(§6.3).

Endianness follows the two lanes §1 states: SPLMSSTR and UG_PART table fields
are little-endian, Parasolid neutral-binary payload fields are big-endian. Each
record states which lane it is in.

## Composite types

| Type | Bytes | Endianness | Meaning |
| ---- | ----: | ---------- | ------- |
| `u48` | 6 | little | 48-bit unsigned little-endian offset word used by the SPLMSSTR header. |
| `xmt_ref` | 2 | big | Parasolid XMT index, small form: a big-endian u16. The large form is a negative i16 remainder plus a u16 quotient and occupies 4 bytes, shifting every later fixed field in the record. |

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `12` | BODY | 24 B | logical fixed record length, before escape and large-index shifts | §4.1 |
| `13` | SHELL | 24 B | logical fixed record length | §4.1 |
| `14` | FACE | 39 B | logical fixed record length | §4.1 |
| `15` | LOOP | 16 B | logical fixed record length | §4.1 |
| `16` | EDGE | 32 B | logical fixed record length | §4.1 |
| `17` | FIN | 23 B | logical fixed record length; FIN has no `node_id` | §4.1 |
| `18` | VERTEX | 28 B | logical fixed record length | §4.1 |
| `19` | REGION | 16 B | logical fixed record length | §4.1 |
| `29` | POINT | 40 B | logical fixed record length | §4.1 |
| `30` | LINE | 67 B | logical fixed record length | §4.1 |
| `31` | CIRCLE | 99 B | logical fixed record length | §4.1 |
| `32` | ELLIPSE | 107 B | logical fixed record length | §4.1 |
| `50` | PLANE | 91 B | logical fixed record length | §4.1 |
| `51` | CYLINDER | 99 B | logical fixed record length | §4.1 |
| `52` | CONE | 115 B | logical fixed record length | §4.1 |
| `53` | SPHERE | 99 B | logical fixed record length | §4.1 |
| `54` | TORUS | 107 B | logical fixed record length | §4.1 |
| `56` | BLEND_SURF | variable | 66 bytes plus escape and large-index shifts | §4.1 |
| `60` | OFFSET_SURF | 31 B | logical fixed record length | §4.1 |
| `124` | B_SURFACE | 23 B | logical fixed record length | §4.1 |
| `133` | TRIMMED_CURVE | variable | 85 bytes plus escape and large-index shifts | §4.1 |
| `134` | B_CURVE | 23 B | logical fixed record length | §4.1 |
| `137` | SP_CURVE | variable | 33 bytes plus escape and large-index shifts | §4.1 |

## `splmsstr_header`

Spec §2 · layout: byte offsets · size: 31 B

Fixed prefix through the `HEADER` marker. The spec's byte map labels 0x1f as the start of the directory entries; the §2 prose and the parser both place `entry_count:u32 LE` there with the entries at 0x23. Recorded in the pull request.

Parsed by:
- `crates/cadmpeg-codec-nx/src/container.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `magic` | `bytes[8]` | little | spec | 0x00..0x07 ASCII "SPLMSSTR" · value `"SPLMSSTR"` |
| 8 | 1 | `version_tag` | `u8` | little | spec | 0x08 version tag, constant 0x06 |
| 9 | 3 | `file_tag` | `u24` | little | spec | 0x09..0x0b file-specific uint24 LE (correlates with file complexity, not footer offset) |
| 12 | 4 | `zero_word` | `u32` | little | spec | 0x0c..0x0f constant 0x00000000 |
| 16 | 1 | `zero_byte` | `u8` | little | spec | 0x10 constant 0x00 |
| 17 | 6 | `footer_offset` | `u48` | little | spec | 0x11..0x16 FOOTER offset, 48-bit LE (points into the FOOTER region near EOF) |
| 25 | 6 | `header_marker` | `bytes[6]` | little | spec | 0x19..0x1e ASCII "HEADER" · value `"HEADER"` |

Unstated regions:

- `23..25` (2 B): Bytes 0x17..0x18. The spec's byte map skips from `0x11..0x16` to `0x19..0x1e` and states nothing for these two bytes; the parser does not read them either.

## `directory_entry`

Spec §2 · layout: byte offsets · size: 4 B

Only the leading count is at a fixed offset; `path[name_len]` and the 16-byte payload follow it. The path begins `/Root` and has length 6 through 128.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `name_len` | `u32` | little | spec | An entry is `name_len:u32 LE, ASCII path[name_len], payload[16]` |

## `directory_file_payload`

Spec §2 · layout: byte offsets · size: 16 B

The 16-byte payload of a directory entry when it names a file. Other payloads remain exact opaque bytes.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `file_offset` | `u64` | little | spec | A file payload is `file_offset:u64 LE, size:u64 LE` |
| 8 | 8 | `size` | `u64` | little | spec | `file_offset:u64 LE, size:u64 LE`, with nonzero size |

## `legacy_ugii_payload_prefix`

Spec §2.4 · layout: byte offsets · size: 9 B

The CFB directory path identifies the NX wrapper; the CFB signature alone is not sufficient.

Parsed by:
- `crates/cadmpeg-codec-nx/src/container.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `marker` | `bytes[2]` | — | spec | 0x00..0x01 bytes `0d 01` |
| 2 | 4 | `product` | `bytes[4]` | — | spec | 0x02..0x05 ASCII `UGII` |
| 6 | 2 | `padding` | `bytes[2]` | — | spec | 0x06..0x07 ASCII two spaces |
| 8 | 1 | `version` | `u8` | — | spec | 0x08 UGII version byte |

## `ug_part_segment_index_row`

Spec §2 · layout: byte offsets · size: 12 B

Row ordinal 1 has `type_code = 1`, `subtype_code = 1`, and a `value` equal to the payload-relative byte offset immediately after the index.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `type_code` | `u32` | little | spec | type_code:u32 subtype_code:u32 value:u32 |
| 4 | 4 | `subtype_code` | `u32` | little | spec | type_code:u32 subtype_code:u32 value:u32 |
| 8 | 4 | `value` | `u32` | little | spec | type_code:u32 subtype_code:u32 value:u32 |

## `fastload_structure_envelope`

Spec §2.3 · layout: byte offsets · size: 12 B

`payload_len + 12` equals the bounded directory-entry size. The payload begins `OM 01 01`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `signature` | `bytes[4]` | — | spec | `ff ff ff ff 00 00 00 00 payload_len:u32 BE` |
| 4 | 4 | `zero_word` | `bytes[4]` | — | spec | `ff ff ff ff 00 00 00 00 payload_len:u32 BE` |
| 8 | 4 | `payload_len` | `u32` | big | spec | payload_len:u32 BE`. `payload_len + 12` equals the bounded directory-entry size |

## `om_section_header`

Spec §7.1 · layout: byte offsets · size: 14 B

Signature-relative. `section_end = signature_offset + 16 + payload_size`, so bytes +14..+16 belong to the header but the spec names no field for them.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `signature` | `bytes[4]` | — | spec | An OM section starts at signature `ff ff ff ff` |
| 8 | 4 | `payload_size` | `u32` | big | spec | stores `payload_size:u32 BE` at `+8` |
| 12 | 2 | `om_marker` | `bytes[2]` | — | spec | Bytes `+12..+14` are `OM`. |

Unstated regions:

- `4..8` (4 B): Bytes +4..+8. The spec describes no field between the signature and the payload size.

## `jt_document_header`

Spec §2.3 · layout: byte offsets · size: 105 B

Byte order is zero and the reserved word is zero. Field offsets are derived by laying the spec's ordered field list out from the header start; the total 105 is the derived sum.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 80 | `version_field` | `bytes[80]` | little | spec | Its header is `version_field[80], byte_order:u8 |
| 80 | 1 | `byte_order` | `u8` | little | derived | Offset derived from the stated 80-byte version field. |
| 81 | 4 | `reserved` | `u32` | little | derived | Offset derived by laying the stated ordered field list out from the header start. |
| 85 | 4 | `toc_offset` | `u32` | little | derived | Offset derived by laying the stated ordered field list out from the header start. |
| 89 | 16 | `lsg_segment_id` | `bytes[16]` | little | derived | Offset derived by laying the stated ordered field list out from the header start. |

## `jt_toc_entry`

Spec §2.3 · layout: byte offsets · size: 28 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `segment_id` | `bytes[16]` | little | spec | `segment_id[16], segment_offset:u32 LE |
| 16 | 4 | `segment_offset` | `u32` | little | spec | segment_offset:u32 LE, segment_byte_len:u32 LE |
| 20 | 4 | `segment_byte_len` | `u32` | little | spec | segment_byte_len:u32 LE, attributes[4] |
| 24 | 4 | `attributes` | `bytes[4]` | little | spec | attributes[4] |

## `jt_shape_lod_element_header`

Spec §2.3 · layout: byte offsets · size: 25 B

`element_byte_len` counts every byte after its own word, so `body` has length `element_byte_len - 21`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `element_byte_len` | `u32` | little | spec | `element_byte_len:u32 LE, object_type_id[16] |
| 4 | 16 | `object_type_id` | `bytes[16]` | little | spec | object_type_id[16], object_base_type:u8 |
| 20 | 1 | `object_base_type` | `u8` | little | spec | object_base_type:u8, object_id:u32 LE |
| 21 | 4 | `object_id` | `u32` | little | spec | object_id:u32 LE, body[] |

## `jt_tristrip_shape_node_family_data`

Spec §2.3 · layout: byte offsets · size: 100 B

Offsets are derived by laying the spec's ordered field list out from the block start; the stated 100-byte total for vertex version 1 confirms the arithmetic. Vertex version 2 appends `version_2_vertex_bindings:u64 LE` and occupies 108 bytes.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `shape_version` | `u16` | little | spec | Its family data is `shape_version:u16 LE = 1, reserved_bounds[6]:f32 LE |
| 2 | 24 | `reserved_bounds` | `f32[6]` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 26 | 24 | `untransformed_bounds` | `f32[6]` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 50 | 4 | `area` | `f32` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 54 | 8 | `vertex_count_range` | `i32[2]` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 62 | 8 | `node_count_range` | `i32[2]` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 70 | 8 | `polygon_count_range` | `i32[2]` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 78 | 4 | `memory_byte_len` | `u32` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 82 | 4 | `compression_level` | `f32` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 86 | 2 | `vertex_version` | `u16` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 88 | 8 | `vertex_bindings` | `u64` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 96 | 1 | `vertex_quantization_bits` | `u8` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 97 | 1 | `normal_quantization_factor` | `u8` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 98 | 1 | `texture_quantization_bits` | `u8` | little | derived | Offset derived by laying the stated ordered field list out from the block start. |
| 99 | 1 | `color_quantization_bits` | `u8` | little | derived | Offset derived by laying the stated ordered field list out from the block start; it closes the stated 100-byte total exactly. |

## `toggle_information_stream`

Spec §2.2 · layout: byte offsets · size: 5 B

Fixed prefix only; `count` members of `byte_len:u16 LE, value:utf8[byte_len]` follow, then a four-byte trailer. `count` covers the members and the trailer.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `version` | `u8` | little | spec | version:u8 = 1 |
| 1 | 4 | `count` | `u32` | little | spec | count:u32 LE |

## `extrefstream_header`

Spec §2.3 · layout: byte offsets · size: 20 B

The spec's field list places the record region at byte 20. The parser expects the record region's leading `0x00` at byte 24 and the first directory pair at 25, leaving bytes 20..24 undescribed. Recorded in the pull request.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 12 | `magic` | `bytes[12]` | little | derived | Width derived from the length of the stated ASCII magic. |
| 12 | 4 | `version` | `u32` | little | spec | `version:u32 LE (3)` |
| 16 | 4 | `payload_size` | `u32` | little | spec | `payload_size:u32 LE`, a record region |

## `extrefstream_handle_set_record`

Spec §9.1 · layout: byte offsets · size: 25 B

Fixed prefix only. `count - 1` occurrences of `e0 + handle:u32 BE` follow at +25, then a closing byte equal to `count`. Note the mixed lane: `n` is big-endian while the four ID slots are little-endian.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `lead` | `bytes[4]` | little | spec | begins `01 00 00 00`, then `n:u16 BE` |
| 4 | 2 | `n` | `u16` | big | spec | then `n:u16 BE`, `01`, four `u32 LE` ID slots |
| 6 | 1 | `marker_a` | `u8` | little | derived | Offset derived by laying the stated ordered field list out from the record start. |
| 7 | 16 | `id_slots` | `u32[4]` | little | derived | Offset derived by laying the stated ordered field list out from the record start. |
| 23 | 1 | `marker_b` | `u8` | little | derived | Offset derived by laying the stated ordered field list out from the record start. |
| 24 | 1 | `count` | `u8` | little | derived | Offset derived by laying the stated ordered field list out from the record start. |

Cross-checked against code:

- `crates/cadmpeg-codec-nx/src/container.rs` — The parser derives the same prefix, with the closing byte at +25 + 5 * handle_token_count.

## `analytic_common_header`

Spec §5.1 · layout: byte offsets · size: 19 B

Record-relative, after shifts. Each extended reference in the five-reference common header shifts the analytic payload and record end by two bytes, and the shifts accumulate.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 8 | 2 | `attributes` | `xmt_ref` | — | spec | `attributes +8`, `owner +10` |
| 10 | 2 | `owner` | `xmt_ref` | — | spec | `owner +10`, `next +12` |
| 12 | 2 | `next` | `xmt_ref` | — | spec | `next +12`, `previous +14` |
| 14 | 2 | `previous` | `xmt_ref` | — | spec | `previous +14`, `group +16` |
| 16 | 2 | `group` | `xmt_ref` | — | spec | `group +16`, `sense +18` |
| 18 | 1 | `sense` | `u8` | — | spec | `sense +18` |

Unstated regions:

- `0..8` (8 B): Type tag, XMT index, and (for types carrying one) the `node_id:u32` at record offset +4. The spec states the common header only from +8.

## `face_node`

Spec §5.1 · layout: byte offsets · size: 39 B

Record-relative, after shifts. Unannotated fields are two-byte XMT references. FACE `tolerance` decodes as the sentinel `-3.14158e13` when unset. Any fixed record may place an envelope escape byte `ff` between its type and XMT fields, shifting every logical payload offset by one.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 8 | 2 | `attributes` | `xmt_ref` | big | spec | \| FACE (14) \| `attributes +8`, `tolerance:f64 +10` |
| 10 | 8 | `tolerance` | `f64` | big | spec | `tolerance:f64 +10`, `next_face +18` |
| 18 | 2 | `next_face` | `xmt_ref` | big | spec | `next_face +18`, `prev_face +20` |
| 20 | 2 | `prev_face` | `xmt_ref` | big | spec | `prev_face +20`, `loop +22` |
| 22 | 2 | `loop` | `xmt_ref` | big | spec | `loop +22`, `shell +24` |
| 24 | 2 | `shell` | `xmt_ref` | big | spec | `shell +24`, `surface +26` |
| 26 | 2 | `surface` | `xmt_ref` | big | spec | `surface +26`, `sense +28` |
| 28 | 1 | `sense` | `u8` | big | spec | `sense +28`, `next_on_surface +29` |
| 29 | 2 | `next_on_surface` | `xmt_ref` | big | spec | `next_on_surface +29`, `prev_on_surface +31` |
| 31 | 2 | `prev_on_surface` | `xmt_ref` | big | spec | `prev_on_surface +31`, `next_front +33` |
| 33 | 2 | `next_front` | `xmt_ref` | big | spec | `next_front +33`, `prev_front +35` |
| 35 | 2 | `prev_front` | `xmt_ref` | big | spec | `prev_front +35`, `front_shell +37` |
| 37 | 2 | `front_shell` | `xmt_ref` | big | spec | `front_shell +37` |

Unstated regions:

- `0..8` (8 B): Type tag, XMT index, and the `node_id:u32` at record offset +4 (§5.1 states `Types carrying 'node_id:u32' place it at record offset '+4'`).

## `edge_node`

Spec §5.1 · layout: byte offsets · size: 32 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 8 | 2 | `attributes` | `xmt_ref` | big | spec | \| EDGE (16) \| `attributes +8`, `tolerance:f64 +10` |
| 10 | 8 | `tolerance` | `f64` | big | spec | `tolerance:f64 +10`, `fin +18` |
| 18 | 2 | `fin` | `xmt_ref` | big | spec | `fin +18`, `prev_edge +20` |
| 20 | 2 | `prev_edge` | `xmt_ref` | big | spec | `prev_edge +20`, `next_edge +22` |
| 22 | 2 | `next_edge` | `xmt_ref` | big | spec | `next_edge +22`, `curve +24` |
| 24 | 2 | `curve` | `xmt_ref` | big | spec | `curve +24`, `next_on_curve +26` |
| 26 | 2 | `next_on_curve` | `xmt_ref` | big | spec | `next_on_curve +26`, `prev_on_curve +28` |
| 28 | 2 | `prev_on_curve` | `xmt_ref` | big | spec | `prev_on_curve +28`, `owner +30` |
| 30 | 2 | `owner` | `xmt_ref` | big | spec | `owner +30` |

Unstated regions:

- `0..8` (8 B): Type tag, XMT index, and the `node_id:u32` at record offset +4.

## `fin_node`

Spec §5.1 · layout: byte offsets · size: 23 B

FIN has no `node_id`, so its field block starts at +4 rather than +8.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 4 | 2 | `attributes` | `xmt_ref` | big | spec | \| FIN (17) \| `attributes +4`, `loop +6` |
| 6 | 2 | `loop` | `xmt_ref` | big | spec | `loop +6`, `forward_fin +8` |
| 8 | 2 | `forward_fin` | `xmt_ref` | big | spec | `forward_fin +8`, `backward_fin +10` |
| 10 | 2 | `backward_fin` | `xmt_ref` | big | spec | `backward_fin +10`, `vertex +12` |
| 12 | 2 | `vertex` | `xmt_ref` | big | spec | `vertex +12`, `other_fin +14` |
| 14 | 2 | `other_fin` | `xmt_ref` | big | spec | `other_fin +14`, `edge +16` |
| 16 | 2 | `edge` | `xmt_ref` | big | spec | `edge +16`, `curve +18` |
| 18 | 2 | `curve` | `xmt_ref` | big | spec | `curve +18`, `next_at_vertex +20` |
| 20 | 2 | `next_at_vertex` | `xmt_ref` | big | spec | `next_at_vertex +20`, `sense +22` |
| 22 | 1 | `sense` | `u8` | big | spec | `sense +22` |

Unstated regions:

- `0..4` (4 B): Type tag and XMT index. FIN carries no `node_id`, so its field block starts at +4.

## `vertex_node`

Spec §5.1 · layout: byte offsets · size: 28 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 8 | 2 | `attributes` | `xmt_ref` | big | spec | \| VERTEX (18) \| `attributes +8`, `fin +10` |
| 10 | 2 | `fin` | `xmt_ref` | big | spec | `fin +10`, `prev_vertex +12` |
| 12 | 2 | `prev_vertex` | `xmt_ref` | big | spec | `prev_vertex +12`, `next_vertex +14` |
| 14 | 2 | `next_vertex` | `xmt_ref` | big | spec | `next_vertex +14`, `point +16` |
| 16 | 2 | `point` | `xmt_ref` | big | spec | `point +16`, `tolerance:f64 +18` |
| 18 | 8 | `tolerance` | `f64` | big | spec | `tolerance:f64 +18`, `owner +26` |
| 26 | 2 | `owner` | `xmt_ref` | big | spec | `owner +26` |

Unstated regions:

- `0..8` (8 B): Type tag, XMT index, and the `node_id:u32` at record offset +4.

## `loop_node`

Spec §5.1 · layout: byte offsets · size: 16 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 8 | 2 | `attributes` | `xmt_ref` | big | spec | \| LOOP (15) \| `attributes +8`, `fin +10` |
| 10 | 2 | `fin` | `xmt_ref` | big | spec | `fin +10`, `face +12` |
| 12 | 2 | `face` | `xmt_ref` | big | spec | `face +12`, `next_loop +14` |
| 14 | 2 | `next_loop` | `xmt_ref` | big | spec | `next_loop +14` |

Unstated regions:

- `0..8` (8 B): Type tag, XMT index, and the `node_id:u32` at record offset +4.

## `shell_node`

Spec §5.1 · layout: byte offsets · size: 24 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 4 | 4 | `node_id` | `u32` | big | spec | \| SHELL (13) \| `node_id +4`, `attributes +8` (=1) |
| 8 | 2 | `attributes` | `xmt_ref` | big | spec | `attributes +8` (=1), `body_ref +10` |
| 10 | 2 | `body_ref` | `xmt_ref` | big | spec | `body_ref +10`, `next_shell +12` (=1) |
| 12 | 2 | `next_shell` | `xmt_ref` | big | spec | `next_shell +12` (=1), `first_face +14` |
| 14 | 2 | `first_face` | `xmt_ref` | big | spec | `first_face +14`, sentinels `+16/+18` (=1) |
| 16 | 2 | `sentinel_16` | `xmt_ref` | big | spec | sentinels `+16/+18` (=1), `region_ref +20` |
| 18 | 2 | `sentinel_18` | `xmt_ref` | big | spec | sentinels `+16/+18` (=1), `region_ref +20` |
| 20 | 2 | `region_ref` | `xmt_ref` | big | spec | `region_ref +20`, `face_anchor +22` |
| 22 | 2 | `face_anchor` | `xmt_ref` | big | spec | `face_anchor +22` (`1` or `first_face`) |

Unstated regions:

- `0..4` (4 B): Type tag and XMT index; the spec's field list starts at the `node_id` at +4.

## `point_node`

Spec §5.1 · layout: byte offsets · size: 40 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 8 | 2 | `attributes` | `xmt_ref` | big | spec | \| POINT (29) \| `attributes +8`, `owner +10` |
| 10 | 2 | `owner` | `xmt_ref` | big | spec | `owner +10`, `next +12` |
| 12 | 2 | `next` | `xmt_ref` | big | spec | `next +12`, `prev +14` |
| 14 | 2 | `prev` | `xmt_ref` | big | spec | `prev +14`, `xyz:3×f64 +16` |
| 16 | 24 | `xyz` | `f64[3]` | big | spec | `xyz:3×f64 +16` (meters) |

Unstated regions:

- `0..8` (8 B): Type tag, XMT index, and the `node_id:u32` at record offset +4.

## `line_payload`

Spec §6.1 · layout: byte offsets · size: 67 B

Payload offsets are relative to the record's type tag, after the common header (§5.1). Each point or vector is three f64 BE. The 67-byte total is the §4.1 fixed record length for type 30.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 24 | `point` | `f64[3]` | big | spec | \| LINE (30) \| point `+19`, direction `+43` \| |
| 43 | 24 | `direction` | `f64[3]` | big | spec | point `+19`, direction `+43` |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `circle_payload`

Spec §6.1 · layout: byte offsets · size: 99 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 24 | `center` | `f64[3]` | big | spec | \| CIRCLE (31) \| center `+19`, normal `+43` |
| 43 | 24 | `normal` | `f64[3]` | big | spec | normal `+43`, x_axis `+67`, radius `+91` |
| 67 | 24 | `x_axis` | `f64[3]` | big | spec | x_axis `+67`, radius `+91` |
| 91 | 8 | `radius` | `f64` | big | spec | center `+19`, normal `+43`, x_axis `+67`, radius `+91` |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `ellipse_payload`

Spec §6.1 · layout: byte offsets · size: 107 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 24 | `center` | `f64[3]` | big | spec | \| ELLIPSE (32) \| center `+19`, normal `+43` |
| 43 | 24 | `normal` | `f64[3]` | big | spec | \| ELLIPSE (32) \| center `+19`, normal `+43`, x_axis `+67` |
| 67 | 24 | `x_axis` | `f64[3]` | big | spec | x_axis `+67`, major `+91`, minor `+99` |
| 91 | 8 | `major` | `f64` | big | spec | major `+91`, minor `+99` |
| 99 | 8 | `minor` | `f64` | big | spec | minor `+99` |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `plane_payload`

Spec §6.1 · layout: byte offsets · size: 91 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 24 | `origin` | `f64[3]` | big | spec | \| PLANE (50) \| origin `+19`, normal `+43`, x_axis `+67` \| |
| 43 | 24 | `normal` | `f64[3]` | big | spec | \| PLANE (50) \| origin `+19`, normal `+43`, x_axis `+67` \| |
| 67 | 24 | `x_axis` | `f64[3]` | big | spec | \| PLANE (50) \| origin `+19`, normal `+43`, x_axis `+67` \| |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `cylinder_payload`

Spec §6.1 · layout: byte offsets · size: 99 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 24 | `origin` | `f64[3]` | big | spec | \| CYLINDER (51) \| origin `+19`, axis `+43` |
| 43 | 24 | `axis` | `f64[3]` | big | spec | \| CYLINDER (51) \| origin `+19`, axis `+43`, radius `+67` |
| 67 | 8 | `radius` | `f64` | big | spec | radius `+67`, x_axis `+75` \| |
| 75 | 24 | `x_axis` | `f64[3]` | big | spec | x_axis `+75` \| |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `cone_payload`

Spec §6.1 · layout: byte offsets · size: 115 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 24 | `origin` | `f64[3]` | big | spec | \| CONE (52) \| origin `+19`, axis `+43` |
| 43 | 24 | `axis` | `f64[3]` | big | spec | \| CONE (52) \| origin `+19`, axis `+43`, radius `+67` |
| 67 | 8 | `radius` | `f64` | big | spec | radius `+67`, sin_half `+75` |
| 75 | 8 | `sin_half` | `f64` | big | spec | sin_half `+75`, cos_half `+83` |
| 83 | 8 | `cos_half` | `f64` | big | spec | cos_half `+83`, x_axis `+91` |
| 91 | 24 | `x_axis` | `f64[3]` | big | spec | cos_half `+83`, x_axis `+91` \| |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `sphere_payload`

Spec §6.1 · layout: byte offsets · size: 99 B

Note the slot order: the radius sits between the centre and the axis.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 24 | `center` | `f64[3]` | big | spec | \| SPHERE (53) \| center `+19`, radius `+43` |
| 43 | 8 | `radius` | `f64` | big | spec | radius `+43`, axis `+51` |
| 51 | 24 | `axis` | `f64[3]` | big | spec | axis `+51`, x_axis `+75` |
| 75 | 24 | `x_axis` | `f64[3]` | big | spec | \| SPHERE (53) \| center `+19`, radius `+43`, axis `+51`, x_axis `+75` \| |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `torus_payload`

Spec §6.1 · layout: byte offsets · size: 107 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 24 | `center` | `f64[3]` | big | spec | \| TORUS (54) \| center `+19`, axis `+43` |
| 43 | 24 | `axis` | `f64[3]` | big | spec | \| TORUS (54) \| center `+19`, axis `+43`, major `+67` |
| 67 | 8 | `major` | `f64` | big | spec | major `+67`, minor `+75` |
| 75 | 8 | `minor` | `f64` | big | spec | minor `+75`, x_axis `+83` |
| 83 | 24 | `x_axis` | `f64[3]` | big | spec | x_axis `+83` \| |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `offset_surf_payload`

Spec §6.1 · layout: byte offsets · size: 31 B

The compact partition record ends after `offset_distance`, closing the §4.1 length of 31. The status-framed deltas form continues with one finite `state_scalar:f64 BE` outside this extent.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 1 | `discriminator` | `u8` | big | spec | discriminator byte `+19` (`V`/`I`/`U`) |
| 20 | 1 | `true_offset` | `u8` | big | spec | `true_offset:u8 +20` (`0`/`1`) |
| 21 | 2 | `base_surface` | `xmt_ref` | big | spec | base surface ref `+21`, finite `offset_distance:f64 +23` |
| 23 | 8 | `offset_distance` | `f64` | big | spec | finite `offset_distance:f64 +23` (meters) |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `trimmed_curve_payload`

Spec §6.4 · layout: byte offsets · size: 85 B

A large-index basis-curve reference shifts every later field by two bytes.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 2 | `basis_curve` | `xmt_ref` | big | spec | basis_curve ref `+19` (large-index capable → shifts later fields +2) |
| 21 | 24 | `point_1` | `f64[3]` | big | spec | `point_1 +21`, `point_2 +45` |
| 45 | 24 | `point_2` | `f64[3]` | big | spec | `point_2 +45`, `parm_1:f64 +69` |
| 69 | 8 | `parm_1` | `f64` | big | spec | `parm_1:f64 +69`, `parm_2:f64 +77` |
| 77 | 8 | `parm_2` | `f64` | big | spec | `parm_2:f64 +77` |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `sp_curve_payload`

Spec §6.4 · layout: byte offsets · size: 33 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 2 | `surface` | `xmt_ref` | big | spec | surface ref `+19`, b_curve ref `+21` |
| 21 | 2 | `b_curve` | `xmt_ref` | big | spec | b_curve ref `+21`, original ref `+23` |
| 23 | 2 | `original` | `xmt_ref` | big | spec | original ref `+23`, `tolerance_to_original:f64 +25` |
| 25 | 8 | `tolerance_to_original` | `f64` | big | spec | `tolerance_to_original:f64 +25` (after ref shifts) |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `intersection_type_38`

Spec §6.3 · layout: byte offsets · size: 31 B

§4.1's fixed-record table has no row for type 38; the 31-byte total here is the parser's constant, which the six stated reference offsets close exactly. Recorded in the pull request.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 2 | `ref0_primary_support` | `xmt_ref` | big | spec | six support xmt references at `+19,+21,+23,+25,+27,+29` |
| 21 | 2 | `ref1_second_support_bridge` | `xmt_ref` | big | spec | six support xmt references at `+19,+21,+23,+25,+27,+29` |
| 23 | 2 | `ref2_chart` | `xmt_ref` | big | spec | \| 2 \| `0x28` CHART_s seed/control polyline \| |
| 25 | 2 | `ref3_term_start` | `xmt_ref` | big | spec | \| 3/4 \| `0x29` term_use start / end endpoint \| |
| 27 | 2 | `ref4_term_end` | `xmt_ref` | big | spec | \| 3/4 \| `0x29` term_use start / end endpoint \| |
| 29 | 2 | `ref5_values_array` | `xmt_ref` | big | spec | \| 5 \| `0x00cc` values-array (support UV parameters) \| |

Unstated regions:

- `0..19` (19 B): Type tag, XMT index, `node_id`, and the §5.1 common header through `sense +18`.

## `chart_s_preamble`

Spec §6.3 · layout: byte offsets · size: 52 B

Offsets are relative to `pre`, the end of the `count` and `xmt` fields. The Hvec block always starts at `pre+52`. Field offsets are derived by laying the spec's ordered field list out from `pre`; the stated `pre+52` block start confirms the arithmetic.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `base_parameter` | `f64` | big | derived | Offset derived by laying the stated ordered field list out from `pre`. |
| 8 | 8 | `base_scale` | `f64` | big | derived | Offset derived by laying the stated ordered field list out from `pre`. |
| 16 | 4 | `chart_count` | `u32` | big | derived | Offset derived by laying the stated ordered field list out from `pre`. |
| 20 | 8 | `chordal_error` | `f64` | big | derived | Offset derived by laying the stated ordered field list out from `pre`. |
| 28 | 8 | `angular_error` | `f64` | big | derived | Offset derived by laying the stated ordered field list out from `pre`. |
| 36 | 16 | `parameter_error` | `f64[2]` | big | derived | Offset derived by laying the stated ordered field list out from `pre`; it closes the stated `pre+52` Hvec block start exactly. |

Cross-checked against code:

- `crates/cadmpeg-codec-nx/src/intersection.rs` — The parser's sentinel matches the stated absent-parameter pair.

## `nurbs_surface_descriptor_prefix`

Spec §6.2 · layout: byte offsets · size: 28 B

Offsets are relative to the type tag after the optional envelope and large-index shift. The prefix ends at the V distinct-knot count; the later reference layout is variable-width.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 4 | 1 | `u_periodic` | `u8` | big | spec | `u_periodic +4`, `v_periodic +5` |
| 5 | 1 | `v_periodic` | `u8` | big | spec | `u_periodic +4`, `v_periodic +5` |
| 6 | 2 | `u_degree` | `u16` | big | spec | `u_degree +6`, `v_degree +8` |
| 8 | 2 | `v_degree` | `u16` | big | spec | `u_degree +6`, `v_degree +8` |
| 10 | 4 | `u_pole_count` | `u32` | big | spec | `u_pole_count +10`, `v_pole_count +14` |
| 14 | 4 | `v_pole_count` | `u32` | big | spec | `u_pole_count +10`, `v_pole_count +14` |
| 18 | 1 | `u_knot_type` | `u8` | big | spec | U/V knot types `+18/+19` |
| 19 | 1 | `v_knot_type` | `u8` | big | spec | U/V knot types `+18/+19` |
| 20 | 4 | `u_distinct_knot_count` | `u32` | big | spec | distinct-knot counts `+20/+24` |
| 24 | 4 | `v_distinct_knot_count` | `u32` | big | spec | distinct-knot counts `+20/+24` |

Unstated regions:

- `0..4` (4 B): Type tag and the encoded XMT identity.

## `nurbs_curve_descriptor_prefix`

Spec §6.2 · layout: byte offsets · size: 21 B

Offsets are relative to the type tag after the optional envelope and large-index shift. The reference lane begins at +21 or +23 depending on the selected descriptor framing.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 4 | 2 | `degree` | `u16` | big | spec | `degree +4`, `pole_count +6` |
| 6 | 4 | `pole_count` | `u32` | big | spec | `pole_count +6`, `dimension +10` |
| 10 | 2 | `dimension` | `u16` | big | spec | `dimension +10` (2=UV, 3=XYZ) |
| 12 | 4 | `distinct_knot_count` | `u32` | big | spec | distinct-knot `+12` |
| 16 | 1 | `knot_type` | `u8` | big | spec | knot type `+16` |
| 17 | 1 | `periodic` | `u8` | big | spec | periodic/closed/rational `+17/+18/+19` |
| 18 | 1 | `closed` | `u8` | big | spec | periodic/closed/rational `+17/+18/+19` |
| 19 | 1 | `rational` | `u8` | big | spec | periodic/closed/rational `+17/+18/+19` |
| 20 | 1 | `curve_form` | `u8` | big | spec | curve form `+20` |

Unstated regions:

- `0..4` (4 B): Type tag and the encoded XMT identity.

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| OM record grammars (§7.1) | §3.3 | About sixty fixed-literal token grammars with sequential field positions but no stated absolute offsets. They are slot layouts over a variable-width compact-index lane, so no byte arithmetic closes. |
| `b5`-style deltas record grammars (§4.2, §9.2-§9.4) | §4.2 | Every reference slot is a variable-width encoded XMT index followed by a status byte, so field positions shift per record. The spec states field order and inline schema-header byte strings, not offsets. |
| Int32 Compressed Data Packet Mk. 2 (§2.3) | §2.3 | A most-significant-bit-first bit stream with `u6` and `u3` sub-fields; there are no byte offsets to state. |
| B_SURFACE and B_CURVE counted payload and reference tails (§6.2) | §6.2 | Types 125-128 and 135-136 carry counted arrays and variable-width reference tails whose element counts and XMT widths drive every later position. The fixed descriptor prefixes are tabulated above. |
