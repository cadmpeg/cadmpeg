<!-- Generated from docs/layouts/cfb.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `cfb` record layouts

Source of truth: [`docs/formats/cfb.md`](../../docs/formats/cfb.md).
Table source: `docs/layouts/cfb.toml`.

The shared directory-entry layout applies to version 3 and version 4. Versioned stream-size interpretation is specified in section 2.

## `directory_entry`

Spec §1 · layout: byte offsets · size: 128 B

Parsed by:
- `crates/cadmpeg-container/src/compound.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 64 | `name` | `bytes[64]` | little | spec | UTF-16 name storage. |
| 64 | 2 | `name_length` | `u16` | little | spec | Even byte count including the null terminator; at most 64. |
| 66 | 1 | `object_type` | `u8` | little | spec | Unallocated=0, storage=1, stream=2, root=5. |
| 67 | 1 | `color` | `u8` | little | spec | Red=0 or black=1. |
| 68 | 4 | `left` | `u32` | little | spec | Left sibling ordinal. |
| 72 | 4 | `right` | `u32` | little | spec | Right sibling ordinal. |
| 76 | 4 | `child` | `u32` | little | spec | Child ordinal. |
| 80 | 16 | `clsid` | `bytes[16]` | little | spec | Storage class identifier. |
| 96 | 4 | `state_bits` | `u32` | little | spec | Application flags. |
| 100 | 8 | `creation_time` | `u64` | little | spec | UTC FILETIME. |
| 108 | 8 | `modified_time` | `u64` | little | spec | UTC FILETIME. |
| 116 | 4 | `start_sector` | `u32` | little | spec | First stream sector. |
| 120 | 8 | `stream_size` | `u64` | little | spec | Stream byte length. |
