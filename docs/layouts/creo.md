<!-- Generated from docs/layouts/creo.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `creo` record layouts

Source of truth: [`docs/formats/creo_prt.md`](../../docs/formats/creo_prt.md).
Table source: `docs/layouts/creo.toml`.

Creo `.prt` is a token stream, not a fixed-record format. `creo_prt.md` states
literal byte offsets in exactly two places — the two type-24 bounded-round
surface bodies in §3.3. The fixed-width Unix-compress and CMNM prefixes in §1
are also tabled as byte layouts. The other ASCII container records are
variable-width delimiters or whitespace-delimited fields.

The rest of what is tabulatable is the PSB primitive encoding: the compact
integer, structural token, and three-byte IEEE-fill tables of §2. Those are
recorded as token inventories. Slot layouts (records that are a fixed ordered
sequence of variable-width tokens) are recorded for the two cleanest cases;
about twenty-five more exist in the specification and are not covered in this
pass.

Endianness: §2.1 states the compact integer is big-endian and §8.4 states the
`92`/`da` DICT forms store big-endian integers. Elsewhere the scalar tokens are
described as reconstructing IEEE-754 byte images without an explicit byte-order
statement, so those fields carry `unstated`.

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `00..7f` | one-byte direct integer | 0 B | the byte itself is the value | §2.1 |
| `80..bf` | two-byte big-endian integer | 1 B | value is `((head - 0x80) << 8) \| XX` | §2.1 |
| `c0..ff` | control or special-token range | variable | control or special-token range on typed paths; in `segtab`, `order_table`, and `ent_tab` these are single-byte null sentinels | §2.1 |
| `e0` | named-record header | variable | `e0 <type> <name>\0` | §2.2 |
| `f8` | array opener | variable | `f8 <count>` | §2.2 |
| `f9` | count-bounded scalar body | variable | `f9 <ndim> <count>`; the field declares exactly `dimensions * count` scalar slots | §2.2 |
| `f7` | entity reference | variable | `f7 <id>` | §2.2 |
| `fb` | array close | 0 B | closes an `f8` array | §2.2 |
| `e2` | nested compound-body opener or continuation | 0 B | opens or continues a nested compound body | §2.2 |
| `e3` | compound close or row terminator | 0 B | meaning depends on context | §2.2 |
| `29` | IEEE-fill, byte0 3F, repeated fill | 2 B | three-byte form: `29 XX YY` reconstructs `(0x3F, XX, YY repeated 6 times)` | §Three-byte IEEE-fill form |
| `2a` | IEEE-fill, byte0 3F, zero fill | 2 B | three-byte form: `2a XX YY` reconstructs `(0x3F, XX, YY 00 00 00 00 00)` | §Three-byte IEEE-fill form |
| `2e` | IEEE-fill, byte0 40, repeated fill | 2 B | three-byte form; `2f 43 00` is 38.0 and `2f 20 00` is 8.0 in the sibling row | §Three-byte IEEE-fill form |
| `2f` | IEEE-fill, byte0 40, zero fill | 2 B | three-byte form | §Three-byte IEEE-fill form |
| `42` | IEEE-fill, byte0 BF, repeated fill | 2 B | three-byte form | §Three-byte IEEE-fill form |
| `43` | IEEE-fill, byte0 BF, zero fill | 2 B | three-byte form | §Three-byte IEEE-fill form |
| `47` | IEEE-fill, byte0 C0, repeated fill | 2 B | three-byte form | §Three-byte IEEE-fill form |
| `48` | IEEE-fill, byte0 C0, zero fill | 2 B | three-byte form; `48 22 00` is -9.0 | §Three-byte IEEE-fill form |

## `unix_compress_header`

Spec §1 · layout: byte offsets · size: 3 B

The low five flag bits give the maximum code width from 9 through 16; bit 7 enables block mode and code 256 clears the dictionary. Codes are packed least significant bit first in code-width-sized byte blocks.

Parsed by:
- `crates/cadmpeg-codec-creo/src/container.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `magic` | `bytes[2]` | — | spec | A section payload beginning `1f 9d <flags>` · value `[31, 157]` |
| 2 | 1 | `flags` | `u8` | — | spec | The low five flag bits give the maximum code width from 9 through 16; bit 7 enables block mode |

## `cmnm_model_name_record`

Spec §1 · layout: byte offsets · size: 11 B

Fixed prefix only; `hhh` bytes of ASCII name follow at +11, then trailing ASCII space padding. A unique valid record supplies the header model filename; a repeated or malformed record does not establish identity. Binary model-data may provide the named `model_name` identity field described in the specification.

Parsed by:
- `crates/cadmpeg-codec-creo/src/container.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `prefix` | `bytes[8]` | — | derived | Width derived from the length of the stated literal `#- CMNM ` including its trailing space. · value `"#- CMNM "` |
| 8 | 3 | `name_length_hex` | `bytes[3]` | — | spec | The three ASCII hexadecimal digits give the filename byte length. |

## `type24_first_coordinate_bounded_round`

Spec §3.3 · layout: byte offsets · size: 50 B

The two diameter endpoints and five extent scalars use the tabulated-cylinder first-coordinate lane, including its positive eight-byte `2d` form. Terminal `18` at offset 49 is the zero-valued sixth extent coordinate.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `opener` | `bytes[2]` | unstated | spec | It begins with `4c b7` |
| 7 | 8 | `first_diameter_endpoint` | `bytes[8]` | unstated | derived | Width derived from the two stated offsets: the endpoint occupies 7 through 15. |
| 15 | 1 | `separator` | `u8` | unstated | spec | `12` at offset 15, the second diameter endpoint at offset 16 |
| 16 | 8 | `second_diameter_endpoint` | `bytes[8]` | unstated | derived | Width derived from the two stated offsets: the endpoint occupies 16 through 24. |
| 24 | 25 | `extent_scalars` | `bytes[25]` | unstated | derived | Width derived from the stated offset 24 and the stated terminal at 49; the five scalars therefore occupy five bytes each in this lane. |
| 49 | 1 | `terminal` | `u8` | unstated | spec | Terminal `18` at offset 49 is the zero-valued sixth extent coordinate. |

Unstated regions:

- `2..7` (5 B): Bytes 2 through 7. The specification states no field between the `4c b7` opener and the first diameter endpoint at offset 7.

Cross-checked against code:

- `crates/cadmpeg-codec-creo/src/surface.rs` — The parser gates on length 50, the `4c b7` opener, `0x12` at 15, and `0x18` at 49, matching every stated offset.

## `type24_segmented_first_coordinate_bounded_round`

Spec §3.3 · layout: byte offsets · size: 56 B

Both diameter endpoints and all six extent coordinates use the tabulated-cylinder first-coordinate lane. Every byte range in this record is stated outright, so the table tiles it with no gap.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `opener` | `u8` | unstated | spec | Byte zero is `18` |
| 1 | 8 | `first_diameter_endpoint` | `bytes[8]` | unstated | spec | the first diameter endpoint occupies bytes 1 through 8 |
| 9 | 7 | `literal_run` | `bytes[7]` | unstated | spec | bytes 9 through 15 are `70 bf e3 4f 05 11 10` |
| 16 | 8 | `second_diameter_endpoint` | `bytes[8]` | unstated | spec | the second diameter endpoint occupies bytes 16 through 23 |
| 24 | 30 | `extent_coordinates` | `bytes[30]` | unstated | spec | six contiguous extent coordinates occupy bytes 24 through 53 |
| 54 | 2 | `trailer` | `bytes[2]` | unstated | spec | The body ends with `f7 19`. |

Cross-checked against code:

- `crates/cadmpeg-codec-creo/src/surface.rs` — The parser gates on length 56, `0x18` at 0, the seven-byte literal at 9..16, and `f7 19` at 54..56, matching every stated range.

## `pcurve_endpoint_body`

Spec §4.1 · layout: ordered slots (no stated byte offsets) · size: not stated

A scalar token occupies one slot and a standalone `12` occupies one zero-valued slot; no other unclaimed byte is permitted. All eight values are finite parameter coordinates in the corresponding face spaces. Slot widths depend on each scalar's token prefix, so the record has no fixed byte size.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `endpoint_a_f0` | `f64[2]` | unstated | spec | The specification describes the scalar tokens as reconstructing IEEE-754 byte images but does not state a byte order for this lane. |
| 1 | `endpoint_a_f1` | `f64[2]` | unstated | spec | The specification states no byte order for this lane. |
| 2 | `endpoint_b_f0` | `f64[2]` | unstated | spec | The specification states no byte order for this lane. |
| 3 | `endpoint_b_f1` | `f64[2]` | unstated | spec | The specification states no byte order for this lane. |

Cross-checked against code:

- `crates/cadmpeg-codec-creo/src/curve.rs` — The parser reconstructs the eight-slot pcurve endpoint body as `[f64; 8]`.

## `local_sys_support_frame`

Spec §3.4 · layout: ordered slots (no stated byte offsets) · size: not stated

Reused by plane, cylinder, cone, and torus prototypes, by curve-equation frames, and by coordinate-system records. Slot widths depend on each scalar's token prefix, so the record has no fixed byte size.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `direction_0` | `f64[3]` | unstated | spec | The specification states no byte order for this lane. |
| 1 | `direction_1` | `f64[3]` | unstated | spec | The specification states no byte order for this lane. |
| 2 | `direction_2` | `f64[3]` | unstated | spec | The specification states no byte order for this lane. |
| 3 | `origin` | `f64[3]` | unstated | spec | The specification states no byte order for this lane. |

Cross-checked against code:

- `crates/cadmpeg-codec-creo/src/scalar.rs` — The parser expands the local-system support frame as twelve `f64` slots.

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| UGC container header and TOC rows (§1, §1.2) | §1.2 | ASCII, whitespace-delimited. The legacy P_OBJECT schema and body are variable-width. The TOC header declares a fixed row width, but the specification states the field order inside a row rather than its column positions, so neither a byte nor a column layout can be stated. |
| Surface bodies of §3.2 and §3.3 other than the two type-24 bounded rounds | §3.3 | About twenty positional cylinder variants plus plane and type-26 forms. The specification identifies them by opener bytes and arithmetic invariants over decoded scalars, not by field offsets; they are recognition predicates rather than layouts. |
| Seven-byte DICT scalar forms (§2.3, Seven-byte DICT form) | §Seven-byte DICT form | About ninety prefix-to-IEEE-prefix mappings stated as prose rather than a table, and the specification states outright that each record grammar defines the DICT lane for its scalar slots, so a single flat table would be unsound. |
| Section, feature, and DEPDB record grammars (§5-§8) | §5 | Variable-length token grammars over self-delimiting fields. About twenty-five of them state a complete ordered slot list and are transcribable as slot layouts; they are not covered in this pass. The rest are named but unspecified, and their gaps are already tracked in `creo_prt-open-items.md`. |
