<!-- Generated from docs/layouts/step.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `step` record layouts

Source of truth: [`docs/formats/step.md`](../../docs/formats/step.md).
Table source: `docs/layouts/step.toml`.

STEP has no record layout to tabulate. It is an ISO 10303-21 clear-text
grammar: no magic bytes, no fixed-width fields, no column positions, no
endianness, and no record sizes anywhere in `docs/formats/step.md` or
`crates/cadmpeg-codec-step/`. The specification states the rule outright — a lexer
never assigns line-based meaning to a token — and gives the layout entirely as
EBNF.

The one bit-level rule in the format is the binary literal's nibble packing,
recorded below as a slot layout because it is the only place the specification
fixes a field's position and width. Everything else is listed as not
applicable, with the reason.

## `binary_literal`

Spec §3 · layout: ordered slots (no stated byte offsets) · size: not stated

Nibble-level, not byte-level: positions are hexadecimal digits inside a quoted literal, not bytes in a record. Recorded because it is the only fixed-position field the format defines.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `indicator` | `u8` | n/a | spec | One hexadecimal digit, value 0 through 3. Single nibble, so no byte order applies. |
| 1 | `payload` | `text` | — | spec | Hexadecimal digits, most-significant nibble first. An empty payload uses indicator zero. |

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| Exchange structure | §2 | N/A, text grammar. The exchange structure is an EBNF production over keyword-delimited sections. Nothing in it has a fixed byte position or width. |
| Tokens and values | §3 | N/A, text grammar. Tokens are whitespace- and comment-separated and case-insensitive, and the specification states that a lexer never assigns line-based meaning to a token. |
| Entity parameter slot order | §5 | Positional, but not stated in the specification. Each entity's parameter order is an EXPRESS external-mapping fact carried in the per-entity decoders as literal `.parameter(N)` indices; `step.md` states only the rule that makes such a table well-defined, namely that the partial records in a complex instance are ordered alphabetically by entity name. Tabulating it would mean transcribing the parser rather than the specification, so it is left out of this pass and recorded as a gap. |
| Header section | §6 | N/A, text grammar. The header is a sequence of entity instances with the same token grammar as the data section. |
| Edition 3 sections | §7 | N/A, text grammar. The anchor, reference, and signature sections are keyword-delimited entry lists. The edition 3 ZIP container layout and the Part 26 HDF5 layout are both recorded as open items, not as stated layouts. |
