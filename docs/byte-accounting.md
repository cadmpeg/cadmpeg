# CADIR byte accounting

## Representation

CADIR version 4 adds a top-level `byte_ledger` object. The ledger describes ownership of bytes in one source stream. It contains the source length and an ordered list of nonempty half-open spans.

Each span contains:

- `start`: zero-based inclusive byte offset;
- `end`: zero-based exclusive byte offset;
- `class`: `typed`, `structural`, or `opaque`;
- `owner`: stable format-owned record identity;
- `meaning`: stable machine-readable field or framing name.

An opaque span also contains the identity of the `UnknownRecord` or native record that retains the payload. Retention may store the complete bytes or the exact length and digest under the existing record contract. Typed and structural spans do not duplicate source bytes.

The source length and offsets are unsigned 64-bit integers in memory and JSON. A codec must reject a source whose length cannot be represented. Checked conversion is required before using an offset as a platform allocation or slice index.

## Validation

An absent source has an empty ledger with source length zero. A nonempty source has at least one span. Every span satisfies `start < end <= source_length`. The first span starts at zero, each subsequent span starts at the preceding span's end, and the final span ends at `source_length`. These rules establish complete coverage with no gap or overlap.

`owner` and `meaning` are nonempty. Opaque spans have a nonempty retained-record identity. Typed and structural spans do not have one. Every retained-record identity resolves to exactly one native record or `UnknownRecord`. Multiple adjacent opaque spans may refer to the same record when structural bytes divide its payload.

## Canonicalization

Canonical order is ascending `(start, end, class, owner, meaning)`. Validation rejects input that is not already in source order. Finalization coalesces adjacent spans only when class, owner, meaning, and retained-record identity are equal. Coalescing never crosses a source record boundary represented by a different owner.

## Serialization and migration

`byte_ledger` is required in CADIR version 4, including when empty. Readers continue to accept exactly the current IR version. Version 3 documents migrate by adding an empty ledger only when they have no source stream. A version 3 document with source metadata requires re-decoding from the source because complete ownership cannot be inferred from semantic IR.

Adding the required ledger changes the document schema and increments `ir_version` from `3` to `4`. Native namespace versions do not change solely because their records are referenced by ledger spans.

## Diff behavior

Document diff reports a byte-ledger change independently of semantic arenas. It reports source-length changes and span additions, removals, and modifications keyed by start offset. A span modification lists changes to `end`, `class`, `owner`, `meaning`, and retained-record identity. Semantic equality therefore does not conceal different source ownership.

## Codec contract

Inspection may build an internal ledger without returning CADIR. Decode returns the complete ledger for every successfully framed source, including container-only and lossy semantic decodes. A framing error may return `CodecError` without a ledger. A semantic error after framing retains accounting in the successful `DecodeResult` and records attributable loss.
