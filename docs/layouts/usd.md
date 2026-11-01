<!-- Generated from docs/layouts/usd.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `usd` record layouts

Source of truth: [`docs/formats/usd.md`](../../docs/formats/usd.md).
Table source: `docs/layouts/usd.toml`.

Fixed-offset records of OpenUSD layers. The USDA text encoding is a grammar; the USDC bootstrap header, table-of-contents records, and path-item headers have fixed offsets.

## `usdc_bootstrap`

Spec §9.1 · layout: byte offsets · size: 88 B

Parsed by:
- `crates/cadmpeg-codec-usd/src/detect.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `identifier` | `bytes[8]` | little | spec | \| 0 \| 8 \| identifier, ASCII `PXR-USDC` \| · value `"PXR-USDC"` |
| 8 | 1 | `major` | `u8` | little | spec | \| 8 \| 1 \| major version \| |
| 9 | 1 | `minor` | `u8` | little | spec | \| 9 \| 1 \| minor version \| |
| 10 | 1 | `patch` | `u8` | little | spec | \| 10 \| 1 \| patch version \| |
| 11 | 5 | `version_padding` | `bytes[5]` | little | spec | \| 11 \| 5 \| zero \| |
| 16 | 8 | `toc_offset` | `i64` | little | spec | \| 16 \| 8 \| `int64` byte offset of the table of contents \| |
| 24 | 64 | `reserved` | `bytes[64]` | little | spec | \| 24 \| 64 \| reserved \| |

## `usdc_section`

Spec §9.2 · layout: byte offsets · size: 32 B

Parsed by:
- `crates/cadmpeg-codec-usd/src/usdc/sections.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `name` | `bytes[16]` | little | spec | \| 0 \| 16 \| section name, ASCII, zero-padded \| |
| 16 | 8 | `start` | `i64` | little | spec | \| 16 \| 8 \| `int64` byte offset of the section \| |
| 24 | 8 | `size` | `i64` | little | spec | \| 24 \| 8 \| `int64` byte size of the section \| |

## `usdc_path_item`

Spec §9.5 · layout: byte offsets · size: 12 B

Parsed by:
- `crates/cadmpeg-codec-usd/src/usdc/sections.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `index` | `u32` | little | spec | \| 0 \| 4 \| path index of the item \| |
| 4 | 4 | `element_token` | `u32` | little | spec | \| 4 \| 4 \| element token index \| |
| 8 | 1 | `bits` | `u8` | little | spec | \| 8 \| 1 \| bits \| |
| 9 | 3 | `padding` | `bytes[3]` | little | spec | \| 9 \| 3 \| padding \| |

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| USDA text layer | §3.1 | N/A, text grammar. A text layer is a header line and a grammar of statements; nothing in it has a fixed byte position or width. |
