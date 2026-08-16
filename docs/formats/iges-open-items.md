# IGES open items

This document lists the parts of the IGES format that we do not know. The specification `iges.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

Status requirements for resource-bounded decode, valid semantic output,
complete transfer accounting, semantic writing, target selection, independent
application acceptance, and writer stress are settled in the IGES
specification and support profile. They are not open format items.

# Unrecorded format rules

The items below record decode and write rules that the codec applies and that
neither IGES nor `iges.md` states. They come from a directed sweep of the codec
on 2026-08-08. Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now, with the code that depends on it.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and
  the code. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Use the identifier in commit messages and in code comments. This document uses
ASD-STE100 Simplified Technical English. Record names, field names, and token
values are technical names. They keep their source spelling.

Many items have one shape: the codec refuses an entity because a value misses a
threshold that the codec selected, or because a field is blank where the codec
requires an explicit value. A refusal is not a safe default. It removes geometry
from a conformant file.

## 1. Physical framing and lexical rules

## 2. Global metadata

### GL-02. The units-name comparison rule

**Question.** How is the Global units name compared with the standard unit codes?

**Known.** `global.rs:496-509` uses an exact, case-sensitive byte match and accepts `IN`, `INCH`, `MM`, `FT`, `MI`, `M`, `KM`, `MIL`, `UM`, `CM`, and `UIN`. `global.rs:420-427` refuses units flag `3` when the name is not in that set. The Global section of `iges.md` records this list and its use.

**Need.** We need the standard comparison rule and the complete alias set. A rejected spelling removes the length factor used by geometry and topology projection.

**Note.** Reopened by the 2026-08-16 audit. The current list is explicit, but the closure did not establish the repository's exact padding, case, or alias policy from the IGES specification or from an exporter-authored witness file.

### GL-03. A missing or zero Global minimum resolution

**Question.** What does an absent, zero, or negative Global minimum resolution mean?

**Known.** `global.rs:441-445` requires a finite nonnegative value; `global.rs:532-537` converts it to model units. The Global section of `iges.md` assigns zero the exact-coincidence meaning, while geometry and topology consumers use the value for their own checks.

**Need.** We need the meaning of zero or omission and one behavior across the codec. Loss messages must identify an invalid resolution, not report a geometry disagreement caused by a missing field.

**Note.** Reopened by the 2026-08-16 audit. The closure validates one project interpretation and documents it, but did not cite the IGES specification for positivity, zero semantics, or the cross-consumer policy.

### GL-04. Byte encoding of Global Hollerith values

**Question.** What character encoding do Global Hollerith values use?

**Known.** `global.rs:90-101` rejects non-ASCII and control bytes in Hollerith payloads. `global.rs:562-571` exposes selected values only after UTF-8 conversion; raw source bytes remain in the retained source image. The Global section of `iges.md` states the counted-byte rule but does not identify a character-set authority for all Global text fields.

**Need.** We need the permitted character set and a retention rule for non-UTF-8 values. Raw-byte preservation does not decide whether a producer's byte sequence is valid IGES text.

**Note.** Reopened by the 2026-08-16 audit. Raw-byte retention and tests establish storage behavior, not the format character-set rule.

## 3. Directory fields, the reference graph, and the native arenas

### DR-04. One malformed subfigure definition blocks every inferred occurrence root

**Question.** How is a top-level product instance identified when a subfigure definition is malformed?

**Known.** `native.rs:4421-4455` retains a Type 308 or 320 definition with an empty or filtered member list and records its sequence as malformed. `native.rs:4525-4543` suppresses all Type 408 and 420 root inference when any definition is malformed. `reader.rs:201-207` reports `occurrence.root-inference-blocked`; tests in `entities/structure/tests.rs:1282-1356` assert the suppression. The Product structure section of `iges.md` states the all-or-nothing root rule.

**Need.** We need a source rule for per-member recovery or for blocking root inference after a malformed definition. The decoder must not fabricate occurrences or silently discard valid independent roots.

**Note.** Reopened by the 2026-08-16 audit. The code and synthetic tests implement a conservative project recovery policy, but the rule that one malformed definition invalidates every root inference is not established from the IGES specification or from an exporter-authored witness file.

### DR-16. Native counted records retain partial prefixes after a malformed nested count

**Question.** What native list state is exposed after a nested counted sequence stops before its declared width?

**Known.** `parameter.rs:148-156` returns `None` when a fixed-width count does not fit the remaining tokens. In contrast, `native.rs:1811-1850` pushes a Type 310 glyph before breaking on an invalid motion count; `native.rs:2106-2162` uses one Type 184 count to locate both member items and later transforms; `native.rs:2275-2336` uses a Type 320 member count to locate later type, designator, template, and connection fields; `native.rs:2623-2648` pushes a Type 302 class before breaking; `native.rs:3062-3133` pushes a Type 322 descriptor before breaking; and `native.rs:3257-3346` clamps Type 406 list counts and can retain partial independent-variable lists. `native.rs:1648-1668` also computes Type 106 tuple availability from all remaining tokens rather than an entity-specific parameter end. `entities/structure.rs:1265-1308` and its Type 322 validation report some parent losses, but the native records have no per-list malformed state.

**Need.** An incomplete nested list must be empty or carry an explicit malformed state. A valid first item followed by an oversized declared count must not look like a complete shorter list to native consumers.

**Note.** New finding from the 2026-08-16 hostile count sweep. The current specification requires an incomplete counted list to be empty and forbids sibling reinterpretation, but native projection exposes partial prefixes or clamped empty lists.

## 4. Geometry carriers and tolerances

### GE-07. The curve parameter-domain convention

**Question.** Which parameter domain does each supported curve entity provide?

**Known.** `entities/offsets.rs:82-116` maps Type 100 from endpoint angles, Type 110 Form 0 to `[0, 1]`, Type 130 from its declared bounds, and Types 102, 106, 112, and 126 to entity-defined neutral intervals. The Geometry section of `iges.md` records the same domains and fallbacks.

**Need.** We need the parameter-domain rule for every supported curve form, including open, closed, and unbounded cases, and evidence for every fallback and affine mapping.

**Note.** Reopened by the 2026-08-16 audit. The centralized mapping and coverage tests do not verify the mapping against the IGES specification or an exporter-authored witness file.

### GE-08. Type 106 duplicate points and closure

**Question.** Which duplicate-point and closure patterns are valid in a Type 106 entity?

**Known.** `entities/copious.rs:58-128` rejects coincident non-endpoint points for Form 63 and treats only the first and last points as an allowed duplicate pair. `entities/copious.rs:310-327` reports losses for endpoint disagreement and forbidden duplicates using Global minimum resolution. The Geometry section of `iges.md` states the same policy.

**Need.** We need the Type 106 form rules for duplicate points and closed paths, including the tolerance and whether source order must be retained.

**Note.** Reopened by the 2026-08-16 audit. The closure made the path policy internally consistent, but did not check the policy against the IGES specification or an exporter-authored witness file.

### GE-09. Type 104 endpoints are not independently authoritative

**Question.** Must Type 104 endpoint coordinates agree with the conic parameters, and what tolerance applies?

**Known.** `entities/conics.rs:384-413` evaluates the coefficient-derived carrier at its selected parameter range and compares both declared endpoints against it using Global minimum resolution. The Geometry section of `iges.md` makes the endpoint agreement rule part of admission.

**Need.** We need the source authority between the analytic coefficients and endpoint fields, and the tolerance for disagreement.

**Note.** Reopened by the 2026-08-16 audit. Endpoint validation is implemented, but the closure did not establish the authority or the tolerance from the IGES specification or from a witness file.

### GE-12. Type 126 property flags against the values

**Question.** Which Type 126 representation flags are authoritative when they disagree with the values?

**Known.** `entities/geometry.rs:1075-1086` validates the four flags, `entities/geometry.rs:1153-1163` compares the polynomial flag with weights, and `entities/geometry.rs:1252-1301` compares the planar flag and normal with the control-point geometry. The current specification records these precedence and rejection rules.

**Need.** We need the precedence of flags, values, and derived ranges for every Type 126 form, plus the required behavior for inconsistent records.

**Note.** Reopened by the 2026-08-16 audit. The consistency checks improve failure reporting, but they do not establish which fields are authoritative in a conformant file.

### GE-18. Native Type 106 tuples use a fallback width for invalid interpretation flags

**Question.** What typed native tuple state is valid when Type 106 IP is absent, outside `1..=3`, or disagrees with the Directory form?

**Known.** `native.rs:1637-1646` reads IP and maps every value other than `1`, `2`, or `3` to tuple start `3` and width `1`. `native.rs:1651-1668` then emits tuples with that fabricated one-value layout. `entities/copious.rs:152-187` and `entities/copious.rs:232-245` reject an invalid or form-disagreeing IP and record an entity loss. The Geometry section of `iges.md` states that IP and Directory form are redundant required constraints, that disagreement is malformed, and that IP does not override form semantics.

**Need.** An invalid interpretation must retain raw tokens and an explicit empty or malformed typed state. It must not produce one-component tuples that a native consumer can mistake for a valid layout.

**Note.** New finding from the 2026-08-16 hostile selection and substitution sweep. The semantic projection rejects the record, but the retained native arena fabricates a tuple layout without a native loss or malformed discriminator.

## 5. Surfaces and topology

### TP-04. The Type 140 offset sign uses a per-kind representative normal

**Question.** Which normal determines the sign of a Type 140 offset indicator?

**Known.** `entities/surfaces.rs:185-210` evaluates a support-surface normal at the bounded midpoint and uses `(0, 0)` when complete bounds are unavailable. `entities/surfaces.rs:1181-1193` refuses an indicator that does not agree with that normal. The Topology section of `iges.md` states this representative-parameter rule.

**Need.** We need the source rule for the offset sign and a representative point that is valid for bounded, unbounded, and varying-normal surfaces.

**Note.** Reopened by the 2026-08-16 audit. The implementation, documentation, and fixtures changed together. Neither midpoint selection nor the `(0, 0)` fallback is established from the IGES specification or from an exporter-authored witness file.

### TP-06. Type 180 Form 1 requires a direct Type 186 operand

**Question.** Does a Type 180 Form 1 Boolean tree accept a Type 186 solid directly, or through a complete operand subtree?

**Known.** `entities/csg.rs:70-123` recursively validates Type 180 operands and requires `has_direct_brep` to match Form 1; a Form 1 accepts a direct Type 186 term in addition to the admitted primitive and Type 430 terms. `entities/csg.rs:360-441` validates postfix structure, recursion, and cycles. The Primitive solids section of `iges.md` records the direct-operand rule.

**Need.** We need the operand rule for Boolean subtrees and the treatment of nested or malformed operands from the IGES specification or exporter-authored witness files.

**Note.** Reopened by the 2026-08-16 audit. The recursive interpretation is internally consistent, but the closure did not verify the rule against the IGES specification or an exporter-authored witness file.

## 6. Product structure, annotation, and presentation

### PS-06. Type 402 Form 5 requires a non-null leader pointer

**Question.** May a Type 402 Form 5 label placement have no leader?

**Known.** `entities/structure.rs:288-297` requires a valid Type 214 pointer for every Form 5 placement. `entities/structure/tests.rs:874-890` rejects a label display without a leader, and the Product structure section of `iges.md` states the non-null requirement.

**Need.** We need the nullability of the Form 5 leader field from the IGES specification or exporter-authored witness files.

**Note.** Reopened by the 2026-08-16 audit. The requirement and fixture coverage are explicit, but the closure did not establish it from the IGES specification or from a witness file.

### PS-07. Type 406 Form 33 requires a file-global unique identity

**Question.** Must a Type 406 Form 33 sheet identifier be unique across the file?

**Known.** `entities/structure.rs:1021-1056` requires the `(number, name)` pair to occur once in the file, requires one Type 404 owner, and requires one sheet property on that owner. `entities/drawing/tests.rs:83-89` rejects duplicate drawing sheet IDs. The Appearance section of `iges.md` states the same identity scope.

**Need.** We need the identity scope and duplicate behavior from the IGES specification or exporter-authored witness files.

**Note.** Reopened by the 2026-08-16 audit. The file-global rule and duplicate test are project decisions; the closure did not establish that scope from the IGES specification or from a witness file.

### PS-08. Type 406 Form 6 requires an ordered layer pair

**Question.** Must the Type 406 Form 6 layer numbers be in ascending order?

**Known.** `entities/structure.rs:298-310` requires nonnegative lower and upper values with `upper >= lower`; the property test rejects a descending pair. The Appearance section of `iges.md` calls the pair ordered.

**Need.** We need the field definitions and order rule for the Form 6 layer pair.

**Note.** Reopened by the 2026-08-16 audit. The check and fixture establish implementation behavior, but the closure did not establish the ordering requirement from the IGES specification.

## 7. Write path

### WR-10. Fixed protocol constants with no complete source mapping

**Question.** What are the correct `PREF`, creation-method, and hierarchy values for generated records?

**Known.** `writer.rs:39-47` fixes frame and pcurve protocol values, and `writer.rs:2502-2506` and `writer.rs:2688-2697` emits them for Type 141 and Type 142. The writer also emits fixed Directory status strings at `writer.rs:1240-1575` and `writer.rs:3870-4505`; the Topology section of `iges.md` records the output constants.

**Need.** We need the correct value for each field, and evidence from the IGES specification or from exporter-authored witness files for the Type 504 hierarchy difference and each Type 141/142 protocol value.

**Note.** Reopened by the 2026-08-16 audit. The constants are explicit and deterministic, but the closure recorded them as settled without a complete mapping to the IGES specification or to exporter-authored witness files.

## 8. Evidence

### EV-02. The independent-application gate has no recorded run

**Question.** Does the FreeCAD acceptance gate pass for the complete writable geometry profile, with the run recorded?

**Known.** `.github/workflows/iges-freecad.yml` defines the gate and uploads both JSON reports as run artifacts. `docs/formats/iges-fixture-charter.md` records an exact 11/11 pass and a broad 37/37 pass. `docs/format-support.md` cites the same numbers.

**Conflict.** The repository's GitHub Actions has no registration and no run of this workflow, so the recorded numbers trace to a local run whose reports are not retained. Commit `5e4a933da` closed this item citing run artifacts that do not exist.

**Need.** Run the workflow — merge the file to the default branch or trigger it from a pull request — confirm the exact and broad passes, retain both JSON report artifacts, and record the run identifier in `docs/formats/iges-fixture-charter.md`.

**Note.** Reopened by the 2026-08-16 audit. The local pass numbers are plausible and the scripts exist; the element the original Need required and the closure did not supply is the recorded, artifact-retaining run.
