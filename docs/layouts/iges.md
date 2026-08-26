<!-- Generated from docs/layouts/iges.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `iges` record layouts

Source of truth: [`docs/formats/iges.md`](../../docs/formats/iges.md).
Table source: `docs/layouts/iges.toml`.

The Binary representation starts with a fixed 80-byte flag. The Fixed and
Compressed ASCII representations use records with an 80-column text envelope,
so their layouts use `kind = "column"`: 1-based inclusive character columns
that must tile the 80-column card exactly.

`iges.md` uses unnumbered headings, so the `section` keys below are the heading
titles rather than section numbers.

Covers the Binary flag, the ordinary card, the Parameter Data card, the
Terminate data area, and both Directory Entry cards. Per-entity parameter slot order is not in the
specification; `corpus/iges-envelope-a.toml` already carries the entity and form
matrix, and the per-entity parameter order lives only in the parser.

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `S` | Start section | variable | first section in the canonical order | §Physical representation |
| `G` | Global section | variable | its data stream is the concatenation of columns 1 through 72 from its cards | §Physical representation |
| `D` | Directory Entry section | variable | two cards per entry, twenty fixed eight-column fields | §Physical representation |
| `P` | Parameter Data section | variable | columns 1-64 are parameter fragments, 65-72 the owning Directory Entry sequence | §Physical representation |
| `T` | Terminate section | variable | four eight-column section counts | §Physical representation |

## `binary_flag`

Spec §Physical representation · layout: byte offsets · size: 80 B

The six one-byte primitive length fields select the bit widths used by the remaining Binary representation. Each displacement counts its section and any following null padding.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `identifier` | `bytes[1]` | — | spec | Byte 1 is ASCII `B`. |
| 1 | 4 | `remaining_byte_count` | `u32` | big | spec | Bytes 2 through 5 are the big-endian unsigned 32-bit value `75` · value `75` |
| 5 | 6 | `primitive_bit_lengths` | `bytes[6]` | — | spec | Bytes 6 through 11 are the one-byte bit lengths `Is`, `Id`, `NXs`, `NFs`, `NXd`, and `NFd`. |
| 11 | 30 | `section_displacements` | `bytes[30]` | — | spec | Six repetitions of a one-byte ASCII section identifier and a big-endian u32 displacement. |
| 41 | 31 | `unassigned` | `bytes[31]` | — | spec | Bytes 42 through 72 are unassigned. |
| 72 | 1 | `section_marker` | `bytes[1]` | — | spec | Byte 73 is ASCII `B` |
| 73 | 6 | `sequence_padding` | `bytes[6]` | — | spec | bytes 74 through 79 are ASCII blanks or zeroes |
| 79 | 1 | `sequence` | `bytes[1]` | — | spec | byte 80 is ASCII `1`. |

Cross-checked against code:

- `crates/cadmpeg-codec-iges/src/representation.rs` — Representation detection validates the six-byte Binary sequence padding as ASCII blanks or zeroes.

## `canonical_card`

Spec §Physical representation · layout: 1-based character columns · size: 80 B

Byte positions are one-based in the specification. The Global data stream is the concatenation of columns 1 through 72 from its cards.

| Columns | Field | Type | Src | Meaning |
| ------- | ----- | ---- | --- | ------- |
| 1-72 | `section_data` | `text` | spec | Card bytes 1 through 72 are section data. |
| 73-73 | `section_marker` | `char` | spec | Byte 73 is the section marker. |
| 74-80 | `sequence` | `text` | spec | Bytes 74 through 80 are the right-aligned decimal sequence field. |

Cross-checked against code:

- `crates/cadmpeg-codec-iges/src/card.rs` — The parser's card width matches the 80-column card.
- `crates/cadmpeg-codec-iges/src/card.rs` — The parser slices the sequence field from zero-based 73..80, which is 1-based columns 74-80.

## `parameter_data_card`

Spec §Physical representation · layout: 1-based character columns · size: 80 B

The only card whose columns 1-72 are split.

| Columns | Field | Type | Src | Meaning |
| ------- | ----- | ---- | --- | ------- |
| 1-64 | `parameter_fragment` | `text` | derived | Columns 1-64 are the remainder of the 1-72 data area once the back-pointer claims 65-72. The Parameter Data section states the same span outright: "Bytes 1 through 64 of Parameter Data cards form parameter fragments." |
| 65-72 | `de_back_pointer` | `text` | spec | Parameter Data cards instead use byte 65 for an ASCII space, bytes 66 through 72 for the right-aligned positive seven-column Directory Entry sequence |
| 73-73 | `section_marker` | `char` | spec | byte 73 for the `P` marker |
| 74-80 | `sequence` | `text` | spec | bytes 74 through 80 for the Parameter Data sequence |

Cross-checked against code:

- `crates/cadmpeg-codec-iges/src/parameter.rs` — The parser reads the back-pointer from zero-based 64..72, which is 1-based columns 65-72.

## `directory_entry_card_1`

Spec §Directory Entry section · layout: 1-based character columns · size: 72 B

Columns 1-72 of the first of the two Directory Entry cards, as nine fixed eight-column fields. Blank numeric fields take their field-defined default; nonblank numeric fields are right-aligned signed decimal integers. Columns 73-80 carry the section marker and sequence field of the enclosing card.

| Columns | Field | Type | Src | Meaning |
| ------- | ----- | ---- | --- | ------- |
| 1-8 | `entity_type` | `text` | spec | The first card fields are entity type, Parameter Data start sequence, structure |
| 9-16 | `parameter_start` | `text` | spec | entity type, Parameter Data start sequence, structure, line font pattern |
| 17-24 | `structure` | `text` | spec | Parameter Data start sequence, structure, line font pattern, level |
| 25-32 | `line_font_pattern` | `text` | spec | structure, line font pattern, level, view |
| 33-40 | `level` | `text` | spec | line font pattern, level, view, transformation matrix |
| 41-48 | `view` | `text` | spec | level, view, transformation matrix, label-display associativity |
| 49-56 | `transformation_matrix` | `text` | spec | view, transformation matrix, label-display associativity, and the eight-character status number |
| 57-64 | `label_display` | `text` | spec | transformation matrix, label-display associativity, and the eight-character status number |
| 65-72 | `status` | `text` | spec | Four two-digit decimal subfields: blank status, subordinate-entity switch, entity-use flag, and hierarchy. |

Cross-checked against code:

- `crates/cadmpeg-codec-iges/src/directory.rs` — The parser splits columns 1-72 into nine eight-column Directory Entry fields.

## `directory_entry_card_2`

Spec §Directory Entry section · layout: 1-based character columns · size: 72 B

Columns 1-72 of the second Directory Entry card. The repeated entity type must equal the first-card value. Reserved bytes are retained whether blank or nonblank.

| Columns | Field | Type | Src | Meaning |
| ------- | ----- | ---- | --- | ------- |
| 1-8 | `entity_type_repeat` | `text` | spec | The second card fields are the repeated entity type, line weight, color |
| 9-16 | `line_weight` | `text` | spec | the repeated entity type, line weight, color, Parameter Data card count |
| 17-24 | `color` | `text` | spec | line weight, color, Parameter Data card count, form number |
| 25-32 | `parameter_line_count` | `text` | spec | color, Parameter Data card count, form number, two reserved fields |
| 33-40 | `form_number` | `text` | spec | Parameter Data card count, form number, two reserved fields, entity label |
| 41-48 | `reserved_1` | `text` | spec | form number, two reserved fields, entity label, and entity subscript |
| 49-56 | `reserved_2` | `text` | spec | two reserved fields, entity label, and entity subscript |
| 57-64 | `entity_label` | `text` | spec | two reserved fields, entity label, and entity subscript |
| 65-72 | `entity_subscript` | `text` | spec | entity label, and entity subscript. The repeated entity type must equal the first-card value. |

## `terminate_data_area`

Spec §Physical representation · layout: 1-based character columns · size: 72 B

Columns 1-72 of the Terminate card. Each field is a section letter plus a seven-digit count. The remaining data area is blank.

| Columns | Field | Type | Src | Meaning |
| ------- | ----- | ---- | --- | ------- |
| 1-8 | `start_count` | `text` | spec | `S` plus the seven-digit Start count |
| 9-16 | `global_count` | `text` | spec | `G` plus the seven-digit Global count |
| 17-24 | `directory_entry_count` | `text` | spec | `D` plus the seven-digit Directory Entry card count |
| 25-32 | `parameter_data_count` | `text` | spec | `P` plus the seven-digit Parameter Data card count |
| 33-72 | `blank_remainder` | `text` | spec | The remaining data area is blank. |

Cross-checked against code:

- `crates/cadmpeg-codec-iges/src/card.rs` — The parser takes the first 32 columns of the Terminate card as the four eight-column fields.

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| Global section field order | §Global section | The specification names the Global fields in prose but gives no numbered slot list; the Global data stream is a delimited value list, not a column layout. The only slot numbering that exists is the parser's index into the delimited values. |
| Per-entity Parameter Data slot order | §Parameter Data section | The specification pins no fixed layout for entity parameter ordering; it lives in the per-entity decoders. The entity and form matrix is already machine-encoded in `corpus/iges-envelope-a.toml`. |
