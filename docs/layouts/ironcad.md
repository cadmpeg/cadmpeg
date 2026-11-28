<!-- Generated from docs/layouts/ironcad.toml by
     crates/cadmpeg-layout-tables/tests/layout_tables/generate.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg-layout-tables`. -->

# `ironcad` record layouts

Source of truth: [`docs/formats/ironcad.md`](../../docs/formats/ironcad.md).
Table source: `docs/layouts/ironcad.toml`.

Framed compound streams carry a fixed 20-byte header and one or more zlib
blocks (§2). The `Brep Data` payload is a tree of u64-length nodes and u32-length
kernel records; only the record header has fixed offsets (§3).

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `0x80000001` | KERNEL_ACIS | variable | Kernel record holding one ACIS binary save stream. | §3 |
| `0x80000002` | KERNEL_PARASOLID | variable | Kernel record holding one little-endian Parasolid transmit. | §3 |

## `frame_header`

Spec §2 · layout: byte offsets · size: 20 B

Parsed by:
- `crates/cadmpeg-codec-ironcad/src/frame.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `magic` | `u32` | little | spec | \| 0 \| 4 \| `magic` \| `0x3de3de00`. \| · value `1038343680` |
| 4 | 4 | `block_size` | `u32` | little | spec | \| 4 \| 4 \| `block_size` \| `0x40000`. · value `262144` |
| 8 | 8 | `payload_length` | `u64` | little | spec | \| 8 \| 8 \| `payload_length` \| |
| 16 | 4 | `compressed_length` | `u32` | little | spec | \| 16 \| 4 \| `compressed_length` \| |

## `tree_node_header`

Spec §3 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-ironcad/src/tree.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `length` | `u64` | little | spec | A **node** starts with a little-endian `u64` length. |

## `kernel_record_header`

Spec §3 · layout: byte offsets · size: 28 B

Parsed by:
- `crates/cadmpeg-codec-ironcad/src/tree.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `length` | `u32` | little | spec | \| 0 \| 4 \| `length` \| |
| 4 | 4 | `marker` | `u32` | little | spec | \| 4 \| 4 \| `marker` \| Low byte `0x01`; the upper 24 bits vary between records. \| |
| 8 | 4 | `kernel` | `u32` | little | spec | \| 8 \| 4 \| `kernel` \| |
| 12 | 12 | `words` | `u32[3]` | little | spec | \| 12 \| 12 \| `words` \| |
| 24 | 4 | `reserved` | `u32` | little | spec | \| 24 \| 4 \| `reserved` \| `0`, `1`, or `2`. \| |

## `scene_object_header`

Spec §5 · layout: byte offsets · size: 20 B

Parsed by:
- `crates/cadmpeg-codec-ironcad/src/placement.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `version` | `u32` | little | spec | \| 0 \| 4 \| `version` \| |
| 4 | 4 | `class` | `u32` | little | spec | \| 4 \| 4 \| `class` \| |
| 8 | 12 | `class_tail` | `u8[12]` | little | spec | \| 8 \| 12 \| `class_tail` \| |

## `scene_record_matrix_short`

Spec §5 · layout: byte offsets · size: 161 B

Parsed by:
- `crates/cadmpeg-codec-ironcad/src/placement.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 32 | `gap` | `u8[32]` | little | spec | \| 0 \| 32 \| `gap` \| Bytes after the `ModelGUID` text. \| |
| 32 | 1 | `flag` | `u8` | little | spec | \| 32 \| 1 \| `flag` \| `1`. \| · value `1` |
| 33 | 128 | `matrix` | `f64[16]` | little | spec | \| 33 \| 128 \| `matrix` \| |

## `scene_record_matrix`

Spec §5 · layout: byte offsets · size: 169 B

Parsed by:
- `crates/cadmpeg-codec-ironcad/src/placement.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 32 | `gap` | `u8[32]` | little | spec | \| 0 \| 32 \| `gap` \| Bytes after the `ModelGUID` text. \| |
| 32 | 8 | `words` | `u32[2]` | little | spec | \| 32 \| 8 \| `words` \| |
| 40 | 1 | `flag` | `u8` | little | spec | \| 40 \| 1 \| `flag` \| `1`. \| · value `1` |
| 41 | 128 | `matrix` | `f64[16]` | little | spec | \| 41 \| 128 \| `matrix` \| |

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| Parasolid kernel prologue | §3.1 | A lead byte, then a u16-length description and a u32-length schema token (parasolid.md §5); every later offset depends on those lengths. |
