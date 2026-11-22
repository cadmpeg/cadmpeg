<!-- Generated from docs/layouts/solidedge.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `solidedge` record layouts

Source of truth: [`docs/formats/solidedge.md`](../../docs/formats/solidedge.md).
Table source: `docs/layouts/solidedge.toml`.

## `property_set_header`

Spec §2.1 · layout: byte offsets · size: 16 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/settings.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `zero` | `u32` | little | spec | \| 0 \| `u32` zero word \| `0` \| · value `0` |
| 4 | 2 | `tag` | `u16` | little | spec | \| 4 \| `u16` set tag \| `0x5151` \| · value `20817` |
| 6 | 2 | `form` | `u16` | little | spec | \| 6 \| `u16` set form \| \| |
| 8 | 4 | `index` | `u32` | little | spec | \| 8 \| `u32` set index \| \| |
| 12 | 4 | `count` | `u32` | little | spec | \| 12 \| `u32` record count `N` \| \| |

## `space_map_header`

Spec §3.1 · layout: byte offsets · size: 12 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `magic` | `bytes[4]` | little | spec | \| 0 \| ASCII `sseg` \| · value `"sseg"` |
| 4 | 2 | `item_count` | `u16` | little | spec | \| 4 \| `u16` item count `A` \| |
| 6 | 2 | `bodyless_count` | `u16` | little | spec | \| 6 \| `u16` count of items without a body \| |
| 8 | 2 | `next_serial` | `u16` | little | spec | \| 8 \| `u16` next free serial \| |
| 10 | 2 | `free_count` | `u16` | little | spec | \| 10 \| `u16` free-list count `F` \| |

## `space_map_item`

Spec §3.1 · layout: byte offsets · size: 18 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `length` | `u16` | little | spec | \| 0 \| `u16` length `L` \| |
| 2 | 4 | `object_offset` | `u32` | little | spec | \| 2 \| `u32` object offset in the cluster stream \| |
| 6 | 4 | `object_id` | `u32` | little | spec | \| 6 \| `u32` object identifier \| |
| 10 | 2 | `form_word` | `u16` | little | spec | \| 10 \| `u16` form word; bits 6 and above are the object's form \| |
| 12 | 2 | `word_a` | `u16` | little | spec | \| 12 \| `u16` \| |
| 14 | 2 | `word_b` | `u16` | little | spec | \| 14 \| `u16` \| |
| 16 | 2 | `flags` | `u16` | little | spec | \| 16 \| `u16` flags; bit `0x2000` selects the class rule in §3.3 \| |

## `cluster_header`

Spec §3.1 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `magic` | `u32` | little | spec | A cluster stream starts with the `u32` value `0x6C90F544` · value `1821439300` |
| 4 | 4 | `object_count` | `u32` | little | spec | and a `u32` object |

## `object_frame`

Spec §3.1 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `tag` | `u16` | little | spec | \| 0 \| `u16` `0x5151` \| · value `20817` |
| 2 | 2 | `form` | `u16` | little | spec | \| 2 \| `u16` form, equal to the item's form word shifted right by 6 \| |
| 4 | 4 | `object_id` | `u32` | little | spec | \| 4 \| `u32` object identifier, equal to the item's identifier \| |

## `type_table_section_header`

Spec §3.2 · layout: byte offsets · size: 20 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `magic` | `bytes[4]` | little | spec | header that starts with ASCII `ePyT`. · value `"ePyT"` |
| 10 | 2 | `count` | `u16` | little | spec | The `u16` values at header offsets 10 |
| 14 | 2 | `count_repeat` | `u16` | little | spec | and 14 are equal and give the section's record count. |

Unstated regions:

- `4..10` (6 B): §3.2 states no field between the magic and the count.
- `12..14` (2 B): §3.2 states no field between the two counts.
- `16..20` (4 B): §3.2 states no field after the second count.

## `type_declaration`

Spec §3.2 · layout: byte offsets · size: 30 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `class_guid` | `bytes[16]` | little | spec | \| 0 \| 16-byte class GUID \| |
| 16 | 4 | `index` | `u32` | little | spec | \| 16 \| `u32` declaration index |
| 20 | 2 | `marker_one` | `u16` | little | spec | \| 20 \| `u16` `1` \| · value `1` |
| 22 | 2 | `marker_absent` | `u16` | little | spec | \| 22 \| `u16` `0xFFFF` \| · value `65535` |
| 24 | 4 | `metadata` | `u32` | little | spec | \| 24 \| `u32` metadata word \| |
| 28 | 2 | `name_len` | `u16` | little | spec | \| 28 \| `u16` class-name byte count `n` \| |

## `type_compact_record`

Spec §3.2 · layout: byte offsets · size: 26 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `guid` | `bytes[16]` | little | spec | a 16-byte GUID and ten bytes |
| 18 | 2 | `link` | `u16` | little | spec | `u16` value `V` at record offset 18 |

Unstated regions:

- `16..18` (2 B): §3.2 states no field before the value.
- `20..26` (6 B): §3.2 states no field after the value.
