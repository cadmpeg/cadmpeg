# Source-fidelity byte accounting

## Representation

`SourceFidelity` is a decode-time sidecar that travels beside `CadIr`. Its independently versioned `byte_ledger` describes ownership of bytes in one source stream. The ledger contains the source length and an ordered list of nonempty half-open spans. Byte accounting is not part of the neutral product schema and changes to it do not change `ir_version`.

Each span contains:

- `start`: zero-based inclusive byte offset;
- `end`: zero-based exclusive byte offset;
- `class`: `typed`, `structural`, or `opaque`;
- `owner`: stable format-owned record identity;
- `meaning`: stable machine-readable field or framing name.

An opaque span also contains the identity of a sidecar `retained_record`. Each retained record contains its source stream, half-open range, SHA-256 digest, and optional bytes. Every record named by an opaque span contains complete bytes. Records retained only for source identity or integrity accounting may omit bytes. Typed and structural spans do not duplicate source bytes.

The source length and offsets are unsigned 64-bit integers in memory and JSON. A codec must reject a source whose length cannot be represented. Checked conversion is required before using an offset as a platform allocation or slice index.

## Validation

An absent sidecar has no byte-accounting claim. A sidecar for an absent source has an empty ledger with source length zero. A nonempty source has at least one span. Every span satisfies `start < end <= source_length`. The first span starts at zero, each subsequent span starts at the preceding span's end, and the final span ends at `source_length`. These rules establish complete coverage with no gap or overlap.

`owner` and `meaning` are nonempty. Opaque spans have a nonempty retained-record identity. Typed and structural spans do not have one. Every opaque identity resolves to exactly one sidecar record. The record stream is `source`, its `[offset, offset + byte_len)` range equals the span, `byte_len` equals the retained data length, and its lowercase hexadecimal SHA-256 equals the digest computed from the retained data. Resolution without retained data does not establish recovery.

## Canonicalization

Canonical order is ascending `(start, end, class, owner, meaning)`. Validation rejects input that is not already in source order. Finalization coalesces adjacent spans only when class, owner, meaning, and retained-record identity are equal. Coalescing never crosses a source record boundary represented by a different owner.

## Serialization and versioning

`SourceFidelity.schema_version` versions the complete sidecar, including its ledger, retained records, provenance, and exactness. Cadmpeg IR version 55 excludes the ledger from `CadIr`. Version 54 documents migrate their semantic product content without source accounting; complete ownership requires decoding the source container. Accounting changes increment the sidecar schema version and never `ir_version`.

The reserved product `native.*.unknowns` arenas contain only stable identities and product links. Decoders transfer source offsets, lengths, digests, and retained bytes into `SourceFidelity.retained_records` before returning `DecodeResult`. Encoders that replay or patch a source consume the sidecar explicitly. A source-only retained record has no required product counterpart.

## Diff behavior

`diff_byte_ledger` compares sidecars independently of semantic arenas. It reports source-length changes and span additions, removals, and modifications keyed by start offset. A span modification lists changes to `end`, `class`, `owner`, `meaning`, and retained-record identity. Semantic equality therefore does not imply equal source ownership.

## Codec contract

Inspection may build an internal ledger without returning a sidecar. Decode returns the complete sidecar for every successfully framed source, including container-only and lossy semantic decodes. A framing error may return `CodecError` without a ledger. A semantic error after framing retains accounting in the successful `DecodeResult` and records attributable loss. Validation and proof reporting consume `DecodeResult.source_fidelity`, never a field on `CadIr`.
