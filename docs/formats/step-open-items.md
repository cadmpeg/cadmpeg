# STEP Open Items

This document lists the parts of STEP exchange formats that we do not know. The specification `step.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. External resources

## 3. Containers and other encodings

## 4. Signatures

## 5. Topology and pcurve decisions

### TP-13. Error class of a strict-mode refusal

**Question.** Which error class must a strict-mode refusal use?

**Known.** `crates/cadmpeg-ir/src/codec.rs:274-286` returns
`CodecError::Malformed` for the first loss whose `strict_consequence` is
`Reject`. That code is the generic `impl<C: CodecBackend + ?Sized> Codec for C`,
so each codec gives the same class. `cadmpeg_core::CodecError`
(`crates/cadmpeg-core/src/error.rs:12-48`) writes that variant as
`malformed container: {0}` and holds no variant for a policy refusal.
`crates/cadmpeg-codec-iges/src/reader.rs:212-222` holds a second strict gate
with the same class. These tests pin the variant with
`matches!(error, CodecError::Malformed(_))`:
`crates/cadmpeg-codec-step/src/parse/tests/complex_order.rs:118`,
`crates/cadmpeg-codec-step/src/parse/tests/omitted.rs:147`,
`crates/cadmpeg-codec-step/src/reader/topology/tests/shells.rs:346` and `:731`,
and `strict_decode_rejects_a_substituted_length_uncertainty` in
`crates/cadmpeg-codec-step/src/reader/geometry/tests/units.rs`.

**Need.** A strict refusal reports a mode decision, not a defect in the bytes.
The text `malformed container` tells a reader that the container is
inconsistent, so a caller cannot separate a damaged file from a policy stop.
The answer adds a `CodecError` variant in `cadmpeg-core` and changes each codec
and test that pins `Malformed` for a strict refusal.

### TP-14. Per-relation status of an admitted pcurve

**Question.** Which channel gives the verification status of one admitted
pcurve relation?

**Known.** `crates/cadmpeg-codec-step/src/reader/topology.rs:3116-3155` counts
the admitted relations and keeps 8 of them, and
`crates/cadmpeg-codec-step/src/reader/topology.rs:494` reports the one document
warning that names those 8. The document holds no other record of an admitted
relation. The exactness map (`crates/cadmpeg-ir/src/annotations.rs:13-28`) is
the per-entity trust channel, and it uses a globally unique entity identity as
its key. A pcurve relation is a `PcurveUse` field of a `Coedge`
(`crates/cadmpeg-ir/src/topology.rs:196-205`), not an entity with an identity,
so the map holds no key for one relation. `Exactness`
(`crates/cadmpeg-ir/src/provenance.rs:68-78`) holds `ByteExact`, `Derived`,
`Inferred`, and `Unknown`. An admitted relation transfers the source value
without transformation and its global invariant is unproved, which no value
gives. `ModelDraft::exactness` (`crates/cadmpeg-ir/src/draft.rs:262-275`)
writes an entity value only. The STEP reader writes no exactness entry:
`crates/cadmpeg-codec-step/src/reader/mod.rs:386` builds
`SourceFidelity::default()`.

**Need.** A consumer that repairs or reviews one coedge must know whether its
pcurve relation holds an unproved invariant. The document warning gives a count
and 8 examples, so a relation outside those 8 has no query. The answer adds a
key for one pcurve use and a status value that means transferred and unproved.
Both changes are in `cadmpeg-ir` and reach each codec that writes exactness.

## 6. Units and measures

## 7. Annotation, presentation, and tessellation

## 8. Product structure and placement
