<!-- Generated from docs/layouts/protein.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `protein` record layouts

Source of truth: [`docs/formats/protein.md`](../../docs/formats/protein.md).
Table source: `docs/layouts/protein.toml`.

Covers the 16-byte instance-stream header (§2) and the three 136-byte page
kinds (§3). Schema XML, ZIP members, and the schema-driven property value
block are listed under "Not tabulated". The Inventor compound-stream
`protein_header` envelope is tabulated in `docs/layouts/inventor.toml`.

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `80 00 01 00` | record_start | 128 B | Opens a logical record; the 128-byte body at bytes 8..136 contributes in full. | §3 |
| `80 00 00 00` | continuation | 128 B | Extends the current record with its complete 128-byte body. | §3 |
| `ff ff ff ff` | terminal | variable | Closes the current record; only `used` body bytes contribute. | §3 |

## `instance_stream_header`

Spec §2 · layout: byte offsets · size: 16 B

Parsed by:
- `crates/cadmpeg-protein/src/lib.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `declared_size` | `u32` | little | spec | The first header word is the little-endian u32 page size and equals `0x88` |

Unstated regions:

- `4..16` (12 B): The remaining twelve header bytes are retained; the specification states no field in that region.

## `record_start_page`

Spec §3 · layout: byte offsets · size: 136 B

Parsed by:
- `crates/cadmpeg-protein/src/lib.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 4 | 4 | `marker` | `bytes[4]` | little | spec | `80 00 01 00` at bytes 4..8 · value `[128, 0, 1, 0]` |
| 8 | 128 | `body` | `bytes[128]` | little | spec | Bytes 8 through 135 are the 128-byte page body |

Unstated regions:

- `0..4` (4 B): Bytes 0 through 3 are a prefix; the specification states no field there.

## `continuation_page`

Spec §3 · layout: byte offsets · size: 136 B

Parsed by:
- `crates/cadmpeg-protein/src/lib.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 4 | 4 | `marker` | `bytes[4]` | little | spec | `80 00 00 00` at bytes 4..8 · value `[128, 0, 0, 0]` |
| 8 | 128 | `body` | `bytes[128]` | little | spec | Bytes 8 through 135 are the 128-byte page body |

Unstated regions:

- `0..4` (4 B): Bytes 0 through 3 are a prefix; the specification states no field there.

## `terminal_page`

Spec §3 · layout: byte offsets · size: 136 B

Parsed by:
- `crates/cadmpeg-protein/src/lib.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `marker` | `bytes[4]` | little | spec | `ff ff ff ff` at bytes 0..4 · value `[255, 255, 255, 255]` |
| 4 | 2 | `used` | `u16` | little | spec | the used payload length as a little-endian u16 at offset 4 |
| 8 | 128 | `body` | `bytes[128]` | little | spec | Bytes 8 through 135 are the 128-byte page body |

Unstated regions:

- `6..8` (2 B): Bytes 6 through 7 are a suffix; the specification states no field there.

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| Package ZIP and schema XML | §1 | Text grammar. Schema entries are named ZIP members; UID, Base, and property declarations are XML attributes with no fixed byte offsets. |
| Logical-record value block | §4 | Schema-driven variable-length carriers and connection blocks. Field position depends on the inherited property set. |
