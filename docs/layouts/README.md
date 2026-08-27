# Record-layout tables

One `<format>.toml` per format holds the numbers each specification states:
offsets, field widths, record sizes, endianness. `<format>.md` beside it is the
rendered view the specification links to. Both are validated by
`crates/cadmpeg/tests/layout_tables.rs`.

Prose byte-offset paragraphs drift — against each other, and against the
codecs. A table a test arithmetically closes does not. The division of labour
is: the specification keeps the semantics, the table keeps the numbers.

## Working on a table

```sh
cargo test -p cadmpeg --test layout_tables                       # validate
UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables  # regenerate the .md pages
UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables  # regenerate crate layout.rs files
```

Never edit a `<format>.md` or a generated `src/layout.rs` by hand; regenerate them.

A table edit without regeneration fails the test. After regenerating, rebuild, run the owning crate's tests, and review the diff.

The generated file holds one module per byte-layout record (`LEN` plus one
offset constant per field), a `*_VALUE` constant for each field that declares
`value`, and a `token` module of tag constants. Records that declare a
`[[record.discrepancy]]` are omitted, with the reason in a comment. Slot
records, column records, composite types, widths, and endianness stay in
hand-written code. `[[record.code]]` pins remain only for facts the emitter
cannot express (column slices, discrepant-record arithmetic, arity tables,
scope-relative maps). Parsing functions are not generated.

Decoder code imports a record module by path or alias (`use crate::layout::block_frame_header as block;`). Never glob-import a layout module.

The mapping from table to generated file is the explicit list `GENERATED_LAYOUT_RS` in `crates/cadmpeg/tests/layout_tables.rs`. `step` is not in that list: it has no byte-layout records.

## What the validator enforces

- **Every record and field cites the specification.** A `section` plus an
  `anchor` phrase that must occur inside that section. A field whose numbers
  were invented has nothing to cite and cannot pass.
- **Byte records tile their declared size exactly.** Fields plus explicit
  `[[record.gap]]` entries must cover `[0, size)` with no hole and no overlap.
  A region the specification says nothing about is named as a gap rather than
  skipped, so the unknowns stay visible.
- **Declared discrepancies must be real, and real ones must be declared.** The
  set of arithmetic problems the validator computes must equal the set of
  `[[record.discrepancy]]` blocks. An undeclared overlap fails; so does an
  invented one.
- **Every multi-byte field resolves an endianness**, or sets it to `unstated`
  and says in a note that the specification omits it.
- **Code cross-checks hit real parser source.** `[[record.code]]` asserts a
  literal substring is present in a named file. The path must not contain a
  `tests` segment: a pin is a claim that a parser agrees with the table.
- **`parsed_by` names a parser.** The path must exist and must not be a test
  file. It is a locator, not a substring check.
- **Every `dialects` id is a declared row.** A record may key on
  `docs/dialects.toml` ids; each must exist there. A table cannot key on a
  dialect that does not exist, and a row deleted from the registry breaks
  every table that named it.

`layout_validator_rejects_broken_tables` runs the same validator over
eighteen deliberately-broken fixtures in
`crates/cadmpeg/tests/fixtures/layout-invalid/`, one per rule, so the rules
are proven to fire rather than assumed to.

## Schema

```toml
schema = 1                          # schema version; only 1 exists
format = "f3d"                      # must match the file stem
spec = "docs/formats/f3d.md"        # repo-relative; anchors resolve against it
endianness = "little"               # optional file-wide default
note = "..."                        # optional scope statement
```

### `[[type]]` — composite fixed-width units

A unit the specification treats as one field, such as a tagged chunk. Declaring
it once keeps its width and endianness in a single place.

```toml
[[type]]
name = "sab_ref8"
bytes = 9
endianness = "little"               # required: little | big | n/a
note = "..."                        # required
section = "6.2"                     # optional; checked when present
anchor = "ref/int chunks are 9 bytes"
```

### `[[token]]` — tag inventories

Tag byte to payload width and meaning, for tag-typed streams.

```toml
[[token]]
tag = "0x13"
name = "POSITION"
payload_bytes = 24                  # omit for variable-length payloads
note = "3D point, three f64"
section = "4.1"
anchor = "`0x13` | POSITION | 24 B | 3D point (3×f64)"
```

### `[[record]]` — one record layout

```toml
[[record]]
name = "body"
kind = "byte"                       # byte | slot | column
dialects = ["acis:asm-binaryfile-8"]  # optional; registry ids that read it
section = "6.2"
anchor = "**Body (61 B):**"
size = 61                           # optional; enforced for byte and column
endianness = "little"               # optional record-level default
note = "..."
parsed_by = "crates/cadmpeg-codec-rhino/src/chunks.rs"
```

`parsed_by` is a repo-relative parser path, or an array of them. It locates the
decoder for a stateless reader. It does not verify a substring.

- `byte` — absolute byte offsets. Every field needs `offset` and a fixed width.
- `slot` — an ordered sequence of typed slots with no stated offsets, for
  token streams where field position depends on preceding values. No
  arithmetic is enforced; `size` is recorded as metadata.
- `column` — 1-based inclusive character columns, for fixed-column text records
  such as the IGES card. Columns must tile `1..=size`.

`dialects` names the `docs/dialects.toml` rows whose grammar reads this record
at these offsets. It is a positive claim about those rows and only those rows:
an empty list says the table has not scoped the record, never that the record
is dialect-invariant. A version word carried *inside* a record is not a
dialect — key only on discriminants the document declares once, document-wide.

Version scoping used to live in record names alone (`long_chunk_header_v50`),
where nothing could check it: applying a fixed offset to the wrong version was
invisible to every test. Where a name still carries a version, prefer the name
that states the seam (`long_chunk_header_narrow` / `_wide`, the length-word
width) and let the key carry the versions.

### `[[record.field]]`

```toml
[[record.field]]
name = "chunk3_first_lump"
offset = 34                         # byte records
columns = "1-8"                     # column records
type = "sab_ref8"
endianness = "little"               # optional; falls back to record then file
source = "spec"                     # spec | derived | code
anchor = "`chunk[3]` @+34 = first_lump"
note = "..."                        # required when source = "derived"
value = [0x14, 0x00, 0x06, 0x00, 0x08, 0x00]
```

`value` is the constant the specification states for the field: a byte array,
an ASCII string (`"V5_CFV2\u0000"`), or an integer matching the field type.
The emitter writes `{FIELD}_VALUE` next to the offset constant.

`source = "derived"` means arithmetic over values the section states — a stride,
a preceding field's width, a declared total. The note must say which. It is not
a licence to guess.

Types: `u8` `i8` `u16` `i16` `u24` `u32` `i32` `u64` `i64` `f32` `f64` `char`
`bool8` `enum8`, arrays `f64[3]` / `u16[5]`, opaque `bytes[N]`, and the
variable-length `cstring` `lp_ascii` `lp_utf16` `token_stream` `subrecord`
`array` `text` (rejected inside a byte record). File-local `[[type]]` names are
also valid.

### `[[record.gap]]`

A byte range inside a `byte` record for which the specification states no
field.

```toml
[[record.gap]]
offset = 0
size = 16
note = "Record head plus `chunk[0]`. The spec states no offset here."
```

### `[[record.discrepancy]]`

A contradiction the table records instead of resolving.

```toml
[[record.discrepancy]]
kind = "size_mismatch"              # size_mismatch | overlap
computed = 99                       # size_mismatch: what the fields add up to
declared = 100                      # size_mismatch: what the spec declares
note = "..."                        # required; state both readings, pick neither
```

### `[[record.code]]`

A claim that a parser agrees with the table on a fact the emitter cannot
express. Do not pin a generated offset or value constant: that is navigation,
and belongs in `parsed_by`.

```toml
[[record.code]]
path = "crates/cadmpeg-codec-sldprt/src/brep/intersection.rs"
contains = "let block = preamble + 52;"
note = "The parser anchors the point block after `preamble = body + 6`."
```

### `[[not_applicable]]`

A part of the format that has no tabulatable layout, with the reason. Use it —
"this section is a text grammar" is a finding, not a gap in the work.

```toml
[[not_applicable]]
area = "Procedural intercurves and spline surfaces"
reason = "Variable-length token graphs with recursive subtypes."
section = "7.3"
anchor = "**Cache-first subtype selection**"
```
