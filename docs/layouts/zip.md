<!-- Generated from docs/layouts/zip.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `zip` record layouts

Source of truth: [`docs/formats/zip.md`](../../docs/formats/zip.md).
Table source: `docs/layouts/zip.toml`.

## `central_header`

Spec §1 · layout: byte offsets · size: 46 B

Parsed by:
- `crates/cadmpeg-container/src/archive.rs`

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
