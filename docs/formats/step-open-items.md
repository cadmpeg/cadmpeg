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

### CE-09. Unregistered root name in a schema object identifier

**Question.** Must a schema object identifier whose root component is an
unregistered ASN.1 identifier stay a valid identifier?

**Known.** `step.md` §6 "The ASN.1 identifiers `itu-t` and `ccitt` are root `0`,
`iso` is root `1`, and" gives the five ASN.1 identifiers that name a root, and
gives no root number to each other ASN.1 identifier. `step.md` §6 "CADIR
decision: an object identifier component number outside the range that" makes a
root component number that is not `0`, `1`, or `2` a recoverable defect: the
decode charges `metadata.schema-object-identifier-out-of-range`, and the
identifier stays invalid for a DATA section schema name and for a
`FILE_POPULATION` governing schema.
`crates/cadmpeg-codec-step/src/parse/schema_identifier.rs:226-233` gives the
number of each of the five names, and gives no number to each other name.
`crates/cadmpeg-codec-step/src/parse/schema_identifier.rs:100-117` tests the
range of a component that holds a number only. A root component that is an
ASN.1 identifier has the `Unnumbered` form, so no rule tests it, and
`crates/cadmpeg-codec-step/src/parse/schema_identifier.rs:169-175` gives the
second component no rule because the root number is absent. The identifier
`{ foo 40 }` is therefore valid, and the identifier `{ 3 40 }` is a charged
defect.

**Conflict.** One defect has two dispositions. A root component that is not a
registered root gets a charged warning and stays unadmitted when it is a
number, and gets no warning and full admission when it is a name.

**Need.** A valid identifier is admitted as a DATA section schema name and as a
`FILE_POPULATION` governing schema, so the disposition changes which records the
decode accepts. The decoder holds the evidence that separates the two cases,
because it lists the five names that give a root number. Give the rule that an
unregistered root name obeys, or give the reason that a name and a number that
both fail the root rule must not agree.

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

### UM-06. Unresolved uncertainty measure with a transferred projection

**Question.** Which channel reports a linear uncertainty measure that does not
resolve, when the document projection transfers a value?

**Known.** `crates/cadmpeg-codec-step/src/reader/geometry.rs:3333-3387` counts
each `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` measure that does not resolve. A
measure does not resolve when its record is absent, when it holds no number,
when it holds no unit reference, when its unit is not a length unit and not a
plane-angle unit, or when its scaled value is not finite or is not more than
zero. `crates/cadmpeg-codec-step/src/reader/geometry.rs:3400-3427` adds these
counts across all contexts and gives `LinearUncertainty::Value`, `Empty`, or
`Ambiguous`. Only `Empty` and `Ambiguous` keep the count.
`crates/cadmpeg-codec-step/src/reader/geometry.rs:333-354` reports
`geometry.uncertainty-length-unresolved` for `Empty`, and names the count in the
`geometry.uncertainty-length-ambiguous` note for `Ambiguous`. The `Value` arm
writes `ir.tolerances.linear` and reports nothing. `step.md` §8 "CADIR decision:
`Tolerances.linear` is the document projection of the" gives the same rule: it
asks for a note only when no candidate resolves.

**Need.** A measure that does not resolve is a candidate that cannot compete.
One distinct resolved value therefore does not show that the contexts agree. A
document that declares one value in one context, and a measure that does not
resolve in a second context, transfers the first value and keeps no record of
the second measure. `step.md` §8 "Geometric-consistency checks use the selected
document tolerance as their" makes `Tolerances.linear` the baseline of the
consistency checks, so a caller that repairs or examines the document must know
that a declared measure was dropped. The answer gives the count of measures that
do not resolve to the transferred-value result, or gives the reason that a
dropped measure needs no record.

## 7. Annotation, presentation, and tessellation

## 8. Product structure and placement
