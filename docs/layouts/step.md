<!-- Generated from docs/layouts/step.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `step` record layouts

Source of truth: [`docs/formats/step.md`](../../docs/formats/step.md).
Table source: `docs/layouts/step.toml`.

STEP has no fixed-width record layout to tabulate. It is an ISO 10303-21
clear-text grammar with keyword-delimited sections and EBNF productions.
ASCII control octets are ignored anywhere in the clear-text grammar. The format
fixes field position and width only inside the binary literal's quoted nibble
sequence.

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
| Tokens and values | §3 | N/A, text grammar. Tokens use spaces, print-control directives, and comments; ASCII control octets have no semantic role. |
| Entity parameter slot order | §5 | N/A, text grammar. Complex-instance partial records use ascending entity-name order. Entity-specific parameter positions come from EXPRESS external mapping and vary by entity type. |
| Header section | §6 | N/A, text grammar. The header is a sequence of entity instances with the same token grammar as the data section. |
| Edition 3 sections | §7 | N/A, text grammar. Anchor, reference, and signature sections use keyword-delimited entry lists. The table covers fixed byte-position fields only. |
