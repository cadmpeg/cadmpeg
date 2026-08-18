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

### TP-12. Report granularity for unproved admission

**Question.** At which granularity must the decoder report an unproved pcurve
admission?

**Known.** `crates/cadmpeg-codec-step/src/reader/topology.rs:2339-2359` pushes
one `LossNote` for each admitted pcurve, and each note holds a formatted
message that names the curve, the surface, and the coedge use. The
`Idf/Idflibs/VC0603_SMD.stp` sample gives 418 such notes in a 187 kB report
for a 632 kB source. `cadmpeg check` prints one line for each note.

**Need.** Decide between one note for each relation and one note for each
document. The note count grows with the model, so a large assembly gives a
report that is large and hard to read. If the per-relation form stays, give
the rule in the specification, because a reader cannot see from the note count
how many distinct problems exist.

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

## 6. Units and measures

## 7. Annotation, presentation, and tessellation

## 8. Product structure and placement
