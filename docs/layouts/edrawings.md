<!-- Generated from docs/layouts/edrawings.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `edrawings` record layouts

Source of truth: [`docs/formats/edrawings.md`](../../docs/formats/edrawings.md).
Table source: `docs/layouts/edrawings.toml`.

The wrapper of the `eModel` member, the fixed head of a shell record, and the
range block of a shell have fixed offsets. The model stream is a tag grammar
whose records have their own length rules.

## `wrapper`

Spec §2.1 · layout: byte offsets · size: 47 B

Comment line, `I` record, `B` record, and the `Z` tag. The zlib stream starts at offset 47.

Parsed by:
- `crates/cadmpeg-codec-edrawings/src/stream.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 15 | `comment` | `bytes[15]` | little | spec | \| Comment line \| 15 bytes \| |
| 15 | 5 | `info` | `bytes[5]` | little | spec | \| `I` record \| 5 bytes \| |
| 20 | 26 | `bounds` | `bytes[26]` | little | spec | \| `B` record \| 26 bytes \| |
| 46 | 1 | `body_tag` | `u8` | little | spec | Tag `0x5a`, one zlib stream · value `90` |

## `wrapper_comment`

Spec §2.1 · layout: byte offsets · size: 15 B

Parsed by:
- `crates/cadmpeg-codec-edrawings/src/stream.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `prefix` | `bytes[8]` | little | spec | ASCII `;; HSF V<major>.<minor> ` · value `";; HSF V"` |

Unstated regions:

- `8..15` (7 B): The version number, a space, and the line end byte `0x0a`. The reader reads up to the line end byte.

## `wrapper_info`

Spec §2.1 · layout: byte offsets · size: 5 B

Parsed by:
- `crates/cadmpeg-codec-edrawings/src/stream.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `tag` | `u8` | little | spec | Tag `0x49`, then a `u32` little-endian. · value `73` |
| 1 | 4 | `value` | `u32` | little | spec | Tag `0x49`, then a `u32` little-endian. |

## `wrapper_bounds`

Spec §2.1 · layout: byte offsets · size: 26 B

Parsed by:
- `crates/cadmpeg-codec-edrawings/src/stream.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `tag` | `u8` | little | spec | Tag `0x42`, one flag byte · value `66` |
| 1 | 1 | `flag` | `u8` | little | spec | Tag `0x42`, one flag byte |
| 2 | 12 | `min` | `f32[3]` | little | spec | the minimum corner `x y z` |
| 14 | 12 | `max` | `f32[3]` | little | spec | the maximum corner `x y z` |

## `shell_head`

Spec §3.6 · layout: byte offsets · size: 11 B

The fixed head of a shell record. The vertices follow at offset 11.

Parsed by:
- `crates/cadmpeg-codec-edrawings/src/hsf/walk.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `opcode` | `u8` | little | spec | \| `S`, `0x0e` \| 2 \| · value `83` |
| 1 | 1 | `constant` | `u8` | little | spec | \| `S`, `0x0e` \| 2 \| · value `14` |
| 2 | 4 | `id` | `u32` | little | spec | \| Id \| 4 \| |
| 6 | 1 | `zero` | `u8` | little | spec | \| Zero \| 1 \| Constant. \| · value `0` |
| 7 | 4 | `vertex_count` | `u32` | little | spec | \| N \| 4 \| |

## `shell_face_list_head`

Spec §3.6 · layout: byte offsets · size: 5 B

Parsed by:
- `crates/cadmpeg-codec-edrawings/src/hsf/walk.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `marker` | `u8` | little | spec | \| `0x01`, L \| 5 \| · value `1` |
| 1 | 4 | `length` | `u32` | little | spec | \| `0x01`, L \| 5 \| |

## `shell_normals_head`

Spec §3.6 · layout: byte offsets · size: 7 B

Parsed by:
- `crates/cadmpeg-codec-edrawings/src/hsf/walk.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `marker` | `bytes[3]` | little | spec | \| `01 07 0C`, M \| 7 \| · value `[1, 7, 12]` |
| 3 | 4 | `length` | `u32` | little | spec | \| `01 07 0C`, M \| 7 \| |

## `shell_range_head`

Spec §3.6.1 · layout: byte offsets · size: 24 B

The range block without its K bytes. The K bytes follow at offset 24.

Parsed by:
- `crates/cadmpeg-codec-edrawings/src/hsf/walk.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `opcode` | `u8` | little | spec | `03` starts the range block · value `3` |
| 1 | 2 | `head` | `bytes[2]` | little | spec | `03 01 02` · value `[1, 2]` |
| 3 | 16 | `range` | `f32[4]` | little | spec | four `f32` `u0 v0 u1 v1` |
| 19 | 1 | `marker` | `u8` | little | spec | the byte `08` · value `8` |
| 20 | 4 | `length` | `u32` | little | spec | a `u32` K |

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| Model stream records | §3 | A tag grammar: every opcode has its own length rule, and the record offsets depend on the preceding values. |
