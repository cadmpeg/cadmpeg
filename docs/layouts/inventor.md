<!-- Generated from docs/layouts/inventor.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `inventor` record layouts

Source of truth: [`docs/formats/inventor.md`](../../docs/formats/inventor.md).
Table source: `docs/layouts/inventor.toml`.

Fixed prefixes and descriptors in the supported RSe schema-31, Meta Stream v8, Protein, UFRxDoc, and kernel-carrier envelope.

## `rse_database_prefix`

Spec §2 · layout: byte offsets · size: 56 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `database_id` | `bytes[16]` | little | spec | 16-byte database identifier |
| 16 | 4 | `schema` | `u32` | little | spec | a u32 schema |
| 20 | 8 | `created_by` | `bytes[8]` | little | spec | an 8-byte creation-version tuple |
| 28 | 8 | `created_filetime` | `u64` | little | spec | a u64 creation FILETIME |
| 36 | 8 | `saved_by` | `bytes[8]` | little | spec | an 8-byte save-version tuple |
| 44 | 8 | `saved_filetime` | `u64` | little | spec | a u64 save FILETIME |
| 52 | 4 | `note_code_units` | `u32` | little | spec | a u32-counted UTF-16LE note |

## `bulk_envelope`

Spec §4 · layout: byte offsets · size: 18 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `prefix` | `bytes[16]` | little | spec | a 16-byte prefix |
| 16 | 2 | `form` | `u16` | little | spec | a u16 form |

Cross-checked against code:

- `crates/cadmpeg-codec-inventor/src/rse.rs` — The bulk parser reads the 18-byte fixed envelope before the exact zlib member.

## `meta_body_prefix`

Spec §3 · layout: byte offsets · size: 14 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 14 | `values` | `u16[7]` | little | spec | seven u16 values |

## `meta_type_descriptor`

Spec §3 · layout: byte offsets · size: 28 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `type_id` | `bytes[16]` | little | spec | a 16-byte type identifier |
| 16 | 2 | `field_0_kind` | `u16` | little | spec | two `(u16, u32)` field pairs |
| 18 | 4 | `field_0_value` | `u32` | little | spec | two `(u16, u32)` field pairs |
| 22 | 2 | `field_1_kind` | `u16` | little | spec | two `(u16, u32)` field pairs |
| 24 | 4 | `field_1_value` | `u32` | little | spec | two `(u16, u32)` field pairs |

## `kernel_carrier_header`

Spec §5 · layout: byte offsets · size: 14 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_state` | `u32` | little | spec | u32 header state |
| 4 | 2 | `header_kind` | `u16` | little | spec | u16 header kind |
| 6 | 4 | `header_value` | `u32` | little | spec | u32 header value |
| 10 | 4 | `schema` | `u32` | little | spec | u32 schema |

## `protein_header`

Spec §7 · layout: byte offsets · size: 4 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `payload_len` | `u32` | little | spec | a u32 payload length |

## `ufrx_header`

Spec §8 · layout: byte offsets · size: 4 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `schema` | `u16` | little | spec | a u16 schema |
| 2 | 2 | `section_version_count` | `u16` | little | spec | a u16 section-version count |

## `ufrx_occurrence_prefix`

Spec §8 · layout: byte offsets · size: 20 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `end_string_flag` | `u32` | little | spec | end_string_flag u32 |
| 4 | 4 | `file_reference_id` | `u32` | little | spec | file_reference_id u32 |
| 8 | 4 | `occurrence_id` | `u32` | little | spec | occurrence_id u32 |
| 12 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 16 | 4 | `title_form_or_count` | `u32` | little | spec | title_form_or_count u32 |

## `assembly_occurrence_prefix`

Spec §9 · layout: byte offsets · size: 50 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 4 | `next_reference` | `u32` | little | spec | next_reference u32 |
| 10 | 4 | `flags` | `u32` | little | spec | flags u32 |
| 14 | 4 | `owner_reference` | `u32` | little | spec | owner_reference u32 |
| 18 | 4 | `node_index` | `u32` | little | spec | node_index u32 |
| 22 | 8 | `state` | `i32[2]` | little | spec | state i32[2] |
| 30 | 4 | `relation_marker` | `u32` | little | spec | relation_marker u32 |
| 34 | 4 | `relation_count` | `u32` | little | spec | relation_count u32 |
| 38 | 4 | `ordinal_key` | `u32` | little | spec | ordinal_key u32 |
| 42 | 4 | `related_marker` | `u32` | little | spec | related_marker u32 |
| 46 | 4 | `related_count` | `u32` | little | spec | related_count u32 |
