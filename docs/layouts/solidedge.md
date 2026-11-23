<!-- Generated from docs/layouts/solidedge.toml by
     crates/cadmpeg-layout-tables/tests/layout_tables/generate.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg-layout-tables`. -->

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
