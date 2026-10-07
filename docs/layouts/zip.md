<!-- Generated from docs/layouts/zip.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `zip` record layouts

Source of truth: [`docs/formats/zip.md`](../../docs/formats/zip.md).
Table source: `docs/layouts/zip.toml`.

## `digital_signature`

Spec §3 · layout: byte offsets · size: 6 B

Parsed by:
- `crates/cadmpeg-container/src/archive.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `signature` | `u32` | little | spec | offset 0 |
| 4 | 2 | `data_length` | `u16` | little | spec | offset 4 |

## `central_header`

Spec §1 · layout: byte offsets · size: 46 B

Parsed by:
- `crates/cadmpeg-container/src/archive/entry.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `signature` | `u32` | little | spec | offset 0 |
| 4 | 2 | `producer_version` | `u16` | little | spec | offsets 4, 6, and 8 |
| 6 | 2 | `extraction_version` | `u16` | little | spec | offsets 4, 6, and 8 |
| 8 | 2 | `flags` | `u16` | little | spec | offsets 4, 6, and 8 |
| 10 | 2 | `compression` | `u16` | little | spec | offset 10 |
| 12 | 2 | `time` | `u16` | little | spec | offsets 12 and 14 |
| 14 | 2 | `date` | `u16` | little | spec | offsets 12 and 14 |
| 16 | 4 | `crc32` | `u32` | little | spec | offsets 16, 20, and 24 |
| 20 | 4 | `compressed_length` | `u32` | little | spec | offsets 16, 20, and 24 |
| 24 | 4 | `expanded_length` | `u32` | little | spec | offsets 16, 20, and 24 |
| 28 | 2 | `name_length` | `u16` | little | spec | offsets 28, 30, and 32 |
| 30 | 2 | `extra_length` | `u16` | little | spec | offsets 28, 30, and 32 |
| 32 | 2 | `comment_length` | `u16` | little | spec | offsets 28, 30, and 32 |
| 34 | 2 | `starting_disk` | `u16` | little | spec | offsets 34 and 36 |
| 36 | 2 | `internal_attributes` | `u16` | little | spec | offsets 34 and 36 |
| 38 | 4 | `external_attributes` | `u32` | little | spec | offsets 38 and 42 |
| 42 | 4 | `local_header_position` | `u32` | little | spec | offsets 38 and 42 |

## `local_header`

Spec §2 · layout: byte offsets · size: 30 B

Parsed by:
- `crates/cadmpeg-container/src/archive/entry.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `signature` | `u32` | little | spec | offset 0 |
| 4 | 2 | `extraction_version` | `u16` | little | spec | offsets 4, 6, and 8 |
| 6 | 2 | `flags` | `u16` | little | spec | offsets 4, 6, and 8 |
| 8 | 2 | `compression` | `u16` | little | spec | offsets 4, 6, and 8 |
| 10 | 2 | `time` | `u16` | little | spec | offsets 10 and 12 |
| 12 | 2 | `date` | `u16` | little | spec | offsets 10 and 12 |
| 14 | 4 | `crc32` | `u32` | little | spec | offsets 14, 18, and 22 |
| 18 | 4 | `compressed_length` | `u32` | little | spec | offsets 14, 18, and 22 |
| 22 | 4 | `expanded_length` | `u32` | little | spec | offsets 14, 18, and 22 |
| 26 | 2 | `name_length` | `u16` | little | spec | offsets 26 and 28 |
| 28 | 2 | `extra_length` | `u16` | little | spec | offsets 26 and 28 |

## `end_record`

Spec §3 · layout: byte offsets · size: 22 B

Parsed by:
- `crates/cadmpeg-container/src/archive/probe.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `signature` | `u32` | little | spec | offset 0 |
| 4 | 2 | `disk` | `u16` | little | spec | offsets 4, 6, 8, and 10 |
| 6 | 2 | `directory_disk` | `u16` | little | spec | offsets 4, 6, 8, and 10 |
| 8 | 2 | `disk_entries` | `u16` | little | spec | offsets 4, 6, 8, and 10 |
| 10 | 2 | `entries` | `u16` | little | spec | offsets 4, 6, 8, and 10 |
| 12 | 4 | `directory_size` | `u32` | little | spec | offsets 12 and 16 |
| 16 | 4 | `directory_position` | `u32` | little | spec | offsets 12 and 16 |
| 20 | 2 | `comment_length` | `u16` | little | spec | offset 20 |

## `zip64_end`

Spec §3 · layout: byte offsets · size: 56 B

Parsed by:
- `crates/cadmpeg-container/src/archive/probe.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `signature` | `u32` | little | spec | offset 0 |
| 4 | 8 | `record_size` | `u64` | little | spec | offset 4 |
| 12 | 2 | `producer_version` | `u16` | little | spec | offsets 12 and 14 |
| 14 | 2 | `extraction_version` | `u16` | little | spec | offsets 12 and 14 |
| 16 | 4 | `disk` | `u32` | little | spec | offsets 16 and 20 |
| 20 | 4 | `directory_disk` | `u32` | little | spec | offsets 16 and 20 |
| 24 | 8 | `disk_entries` | `u64` | little | spec | offsets 24, 32, 40, and 48 |
| 32 | 8 | `entries` | `u64` | little | spec | offsets 24, 32, 40, and 48 |
| 40 | 8 | `directory_size` | `u64` | little | spec | offsets 24, 32, 40, and 48 |
| 48 | 8 | `directory_position` | `u64` | little | spec | offsets 24, 32, 40, and 48 |

## `zip64_locator`

Spec §3 · layout: byte offsets · size: 20 B

Parsed by:
- `crates/cadmpeg-container/src/archive/probe.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `signature` | `u32` | little | spec | offset 0 |
| 4 | 4 | `end_record_disk` | `u32` | little | spec | offset 4 |
| 8 | 8 | `end_record_position` | `u64` | little | spec | offset 8 |
| 16 | 4 | `disks` | `u32` | little | spec | offset 16 |
