# IGES open items

IGES L9 is not achieved. The current score is L8. The bounded semantic
writer and its independent-application checks are extras above L8; they do not
close the L9 gate while decode can time out, return invalid `CadIr`, or omit
semantic records from transfer.

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

## P0 — Make decode terminating and resource-bounded

Fixed ASCII decode has exceeded the 30-second per-file guard on multiple
inputs. This is pathological and unacceptable for a production codec. The
decoder must not spend unbounded time in parameter assembly, reference graph
construction, topology projection, or geometric carrier recovery.

Required closure:

- instrument each decode stage and record the dominant cost for a reduced
  reproducer;
- bound every file-declared count, recursive traversal, graph walk, and
  geometry-recovery search with the service resource policy;
- return a deterministic structured resource error when a bound is exceeded;
- add synthesized regression fixtures for each pathological stage; and
- run the bounded full-file gate in CI so a timeout cannot be reported as a
  successful decode.

The item is closed only when every file in the declared envelope reaches a
terminal success or a bounded, classified error within the agreed limit.

## P0 — Decode success must imply valid `CadIr`

The decoder can return success for documents that `cadmpeg check` rejects.
Observed failures include edge parameter ranges outside their canonical curve
domains and edge curve endpoints that do not meet their vertex positions.

Required closure:

- canonicalize or reject carrier domains before committing edges;
- validate edge endpoints, pcurves, topology ownership, and transforms before
  returning decode success;
- commit no partial topology after a failed validation; and
- add synthesized fixtures for each failure class and run decode followed by
  `cadmpeg check` in the regression gate.

The item is closed only when a successful semantic decode is a valid `CadIr`,
not merely a parseable command result.

## P0 — Account for every omitted semantic record

Successful decodes still produce `record_not_typed` and
`material_not_transferred` losses for trimming, display, and other entity
branches. The read profile must not call these branches complete while the
decoder either drops their semantics or cannot prove their preservation.

Required closure:

- assign every unsupported or omitted semantic construct a stable loss code,
  severity, source identity, and retained native record;
- distinguish deliberate native preservation from geometric projection loss;
- make `--no-salvage` reject all losses that can change model, topology, product,
  or document meaning; and
- update the read profile only after loss coverage and validation pass.

## P0 — Re-establish the L9 gate

L9 remains open until bounded decode, valid-IR output, complete loss accounting,
semantic writing, target-version selection, and independent application
acceptance pass together. The bounded writer tests are not evidence that the
full declared read/write envelope passes this gate.

Required closure:

- run decode, validate, convert, and generated-file re-decode as one evaluated
  gate;
- require independent native-application acceptance for every writable
  profile, including edited and source-less documents; and
- keep the support table and codec README at L8 until this gate passes.

## P1 — Exercise the writer under fuzzing and continuous stress

The current IGES fuzz target exercises container detection, inspection, and
decode. It does not exercise semantic planning, target-version emission, or
writer rejection paths.

Required closure:

- add writer fuzz coverage for valid and malformed `CadIr` values;
- cover replay, source-less synthesis, target versions, topology, loss
  rejection, and unsupported native arenas;
- record a reproducible fuzz campaign and retain minimized regressions; and
- run the timeout and validation gates continuously rather than as an
  environment-only check.

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

### PH-03. The boundary between entity parameters and trailing pointer groups

**Question.** Where does an entity's own parameter list stop and its trailing pointer groups start?

**Known.** IGES 5.3 §§2.2.4.5.1–2 state that the type number precedes Index 1, that the entity type/form table supplies the last specified parameter number `NV`, and that the two counted pointer groups follow those parameters before the record delimiter. `parameter.rs::specified_parameter_end` registers Type 110 Forms 0–2 at token 7 and Type 116 Form 0 at token 5. Section §4.16 lists Type 116 X, Y, Z, and the optional display-symbol pointer at indexes 1–4; §§2.2.1 and 2.2.3 require the defaulted slot to remain represented when later groups follow and default the remaining fields at the record delimiter. `parameter.rs::defaulted_trailing_count` accepts the omitted count fields only for a registered boundary, so Type 116 accepts explicit or omitted `PTR`, an association-only suffix with defaulted `NP`, and a property-only suffix with explicit `NA=0`. `native.rs` uses the selected groups for Type 406 ownership and parameter-reference provenance.

**Need.** We need the exact `NV` formula for each remaining supported entity/form pair, including optional or defaulted entity fields. A candidate with multiple valid unregistered boundaries must remain raw and produce the structured ambiguity loss. An unresolved pointer at a proven boundary remains a parameter reference and graph finding; unresolved candidates without a proven boundary remain part of the remaining entity-specific work.

**Note.** The synthesized Type 110 witness in `ambiguous_trailing_pointer_boundary_file` established token 7. Type 116 closure uses the official §4.16 table, the public Open CASCADE `IGESGeom_ToolPoint::ReadOwnParams`/`WriteOwnParams` and `IGESData_IGESWriter::Associativities`/`Properties` paths, the headless FreeCAD export SHA-256 `29c8182fe1d5a3289fc13d48713b9717d7c3ab19517721b606b9b4fcf1920626`, and synthesized explicit-zero, omitted, association-only, and property-only witnesses with SHA-256 values `e78b9d345166bb981ff3d7d083c2e8098895d7884b5853060cced1289e5553da`, `e65b616754437692a1d612de6da861c0a7df107522c06fc89b180b602b910ab8`, `a08ca27312639b231d962fff0c4d6f7d5ebb1fe418cb66936c7534a9049fd367`, `3b805654dcf676ca357b92ec3e5332ea2c687bfb15bd96921b652477ad6abfc9`, `f7e67667a05c5a3b70463123fe3b656ec591fa80021d3d92`, and `0175891d2ee37220fb9d95be7b8da80f903e46af410af6ccb94d492f3ccc3ed9`. The six synthesized witnesses pass fresh inspect/dump/query/check runs with zero findings and losses; the FreeCAD export supplies the explicit-zero `PTR` witness but has an unrelated Type 402 reverse-pointer warning. The corrected `variable_schema_property_forms` fixture and reviewed snapshots record the explicit `NA=0` field. PH-03 remains open for the remaining supported layouts.

### PH-04. A physical line longer than 80 bytes

**Question.** How does a line with more than 80 payload bytes divide into records?

**Known.** `card.rs:164-170` creates a fixed card only when `payload.len() == CARD_WIDTH`; an overlong line remains one opaque record. The current format documentation calls bytes beyond column 80 a separate physical record.

**Need.** We need the fixed-record division rule for overlong input, or a correction to the format documentation. The rule must distinguish a valid card with trailing bytes from a malformed card.

**Note.** Closure audit 2026-08-10: reopened. Commit `de6b6d5bf` changed the documentation and tests together, but the IGES specification has not been cited for converting an overlong line into a card plus a separate record. The current policy can turn malformed input into a valid card with ignored tail bytes.

### PH-05. Disagreement between the declared and actual Parameter Data card count

**Question.** Is the Directory Entry card count or the set of back-pointers authoritative?

**Known.** `parameter.rs:326-350` treats the declared count as a lower bound. More owned cards produce a warning and are consumed; fewer cards reject the entity. The format documentation states that the declared count defines the expected contiguous range.

**Need.** We need the authority rule for both directions of count disagreement. The same producer defect must not be recoverable in one direction and fatal in the other without evidence.

**Note.** Closure audit 2026-08-10: reopened. Commit `316a50b13` changed the behavior and the self-authored fixtures, but did not establish whether the count or the pointers is authoritative.

## 2. Global metadata

### GL-01. Global defaults, and defaults applied to unparseable fields

**Question.** Which Global fields have defaults, what are they, and what does an unparseable Global field mean?

**Known.** `global.rs` supplies defaults for model scale, units flag, maximum line-weight gradations, and version flag in addition to the delimiter defaults. The same conversion path maps blank, omitted, and malformed values to `None`, after which callers apply defaults. `global.rs:228-249` now preserves some omitted-versus-malformed distinctions, but the complete default table and error policy are not settled.

**Need.** We need the Global-field default table from the IGES specification and a rule that separates an omitted field from a malformed field. A wrong units default rescales the complete model.

**Note.** Closure audit 2026-08-10: reopened. Commit `35dd9c3f2` promoted project defaults and synthetic fixtures; agreement with those fixtures is not independent evidence. The external material checked supports some defaults, not the full current table or malformed-field behavior.

### GL-02. The units-name comparison rule

**Question.** How is the Global units name compared with the standard unit codes?

**Known.** IGES 5.3 §2.2.4.3.14 makes field 14 authoritative unless it is `3`. Section 2.2.4.3.15 makes field 15 redundant for the other flags and lists the exact table payloads `IN`, `INCH`, `MM`, `FT`, `MI`, `M`, `KM`, `MIL`, `UM`, `CM`, and `UIN`. When field 14 is `3`, the same section delegates the desired-unit name to MIL-STD-12 or IEEE 260 rather than defining a closed list. Section 4.77 makes Type 316 a per-data-entity property-pointer attachment whose scale applies to that entity's real data; it does not provide a Global flag-3 factor. `global.rs:271-284` now requires a nonempty flag-3 name without claiming that the canonical table is exhaustive. `reader.rs:97-101` refuses semantic projection when the name has no known millimetre factor, while inspection retains the name. `native.rs:3418-3452` retains Type 316 entries and owner identities.

**Need.** We need the complete admissible flag-3 byte namespace from the referenced standards, including case, padding, aliases, and length units. We also need to establish whether any external application contract supplies a file-wide millimetre factor for a custom Global flag-3 name.

**Conflict.** The earlier exact-eleven-name rule was contradicted by the IGES flag-3 cross-reference and by Open CASCADE `IGESData_BasicEditor::SetUnitName`, which accepts a user-defined name when the flag is `3`. The codec cannot assign a neutral millimetre scale to an unresolved name without risking silent geometric rescaling.

**Note.** Closure audit 2026-08-10 reopened the prior exact-list closure because it had no independent format or producer evidence. This pass settles field-14 precedence, the ordinary-flag table, nonempty flag-3 storage, the per-data-entity scope of Type 316, and the CADIR refusal boundary; the flag-3 namespace and any external file-wide factor source remain open. A code-built witness with Type 316 attached to a data entity retains the Type 316 scale and owner but still refuses full semantic projection for the unresolved Global flag-3 name.

## 3. Directory fields, the reference graph, and the native arenas

### DR-04. One malformed subfigure definition promotes its instances to assembly roots

**Question.** How is a top-level product instance identified when a subfigure definition is malformed?

**Known.** `native.rs:3438-3450` drops a complete Type 308 or Type 320 definition when one member fails integer parsing. `native.rs:3515-3519` then infers roots from instances not named by a surviving definition. The current code suppresses root inference when it records a malformed definition, but this is a project recovery policy.

**Need.** We need a source rule for per-member recovery or for blocking root inference after a malformed definition. The decoder must not fabricate occurrences or silently discard valid independent roots.

**Note.** Closure audit 2026-08-10: reopened. Commit `4080057b9` added suppression and a synthetic malformed fixture, but the rule that one malformed member invalidates every root inference is not established from the IGES specification or from witness files.

## 4. Geometry carriers and tolerances

### GE-01. The Type 124 transformation tolerance

**Question.** What tolerance applies when a Type 124 transformation is compared with the canonical transform?

**Known.** The reader uses intervals derived from Global real precision in `transform.rs`, and commit `40b1687ea` replaced a fixed tolerance with that calculation. The fixture perturbs a transform near the same project-selected boundary.

**Need.** We need the IGES specification rule, or exporter-authored witness files, for transform equality and the accepted precision. Internal interval arithmetic proves only the implementation's chosen criterion.

**Note.** Closure audit 2026-08-10: reopened. The code, fixture, and documentation were authored together; this is the promotion-to-spec pattern described by the QA plan.

### GE-02. Unit-vector acceptance

**Question.** What deviation from unit length is valid for a direction vector?

**Known.** `geometry.rs` derives an interval from Global real precision and accepts a vector when the squared length interval contains one. The threshold is exercised by project-generated data.

**Need.** We need the specification rule, or a corpus of exporter-authored witness files, that establishes the accepted numeric deviation and whether a decoder should normalize or reject it.

**Note.** Closure audit 2026-08-10: reopened. Commit `8d4e832c4` replaced the old threshold with a more principled calculation, but did not establish the format tolerance from the specification or witness files.

### GE-03. Type 112 segment continuity

**Question.** What continuity tolerance applies between adjacent Type 112 segments?

**Known.** `splines.rs` uses Global real-precision intervals for adjacent endpoint comparisons. The fixtures exercise the selected interval, not an independently specified producer boundary.

**Need.** We need the continuity rule and its numeric tolerance from the IGES specification or from exporter-authored witness files.

**Note.** Closure audit 2026-08-10: reopened. Commit `2481439ee` established an internal interval calculation, not conformance evidence.

## 5. Surfaces and topology

### TP-01. The Global minimum resolution serves five unrelated roles

**Question.** Which topology and geometry decisions may use Global minimum resolution?

**Known.** `trimming.rs:59-74` derives a coordinate quantum from the largest coordinate and single-precision significance, then takes the maximum with Global minimum resolution. The result is used for ring closure, vertex merging, and stored edge and face tolerances.

**Need.** We need the source meaning of minimum resolution and separate rules for coordinate precision, curve-fit tolerance, topology sewing, and native tolerance fields.

**Note.** Closure audit 2026-08-10: reopened. Commit `6bb0de35f` supplied a formula and synthetic boundary tests, but the use of one value for these five roles is not established from the specification or witness files.

### TP-03. Declared surface parameter subranges are discarded with no loss

**Question.** Must a Type 141/142 surface-boundary use retain its declared surface parameter subrange?

**Known.** `trimming.rs` projects model curves and uses the neutral edge range; the declared surface parameter bounds are not retained in the native-to-IR path. The current documentation calls this intentional.

**Need.** We need the source semantics of the parameter subrange and a decision whether projection may discard it, must preserve it, or must report a loss.

**Note.** Closure audit 2026-08-10: reopened. Commit `8ddb25d46` added bounds handling and tests, but it did not establish whether the bounds are authoritative for the boundary use.

### TP-04. The Type 140 offset sign uses a per-kind representative normal

**Question.** Which normal determines the sign of a Type 140 offset indicator?

**Known.** `surfaces.rs:206-225` uses the support surface's bounds midpoint when finite and otherwise `(0, 0)` as the representative parameter, then evaluates a normal. The current documentation states this rule.

**Need.** We need the source rule for the offset sign and a representative point that is valid for bounded, unbounded, and varying-normal surfaces.

**Note.** Closure audit 2026-08-10: reopened. Commit `23554c501` changed implementation, documentation, and fixtures together. Neither midpoint selection nor the `(0, 0)` fallback is established from the specification or witness files.

### TP-06. Type 180 Form 1 requires a direct Type 186 operand

**Question.** Does a Type 180 Form 1 Boolean tree accept a Type 186 solid directly, or through a complete operand subtree?

**Known.** `brep.rs` recursively checks Type 180 and Type 430 references and accepts a Form 1 operand when the complete referenced subtree contains a Type 186. The current fixtures use project-generated nested trees.

**Need.** We need the operand rule for Boolean subtrees and the treatment of nested or malformed operands from the IGES specification or exporter-authored witness files.

**Note.** Closure audit 2026-08-10: reopened. Commit `34861ac75` made the recursive interpretation internally consistent, but the source rule remains unverified. The current documentation promotes the recursive choice to a settled format fact.

### TP-09. A model-curve pointer selects the first neutral edge

**Question.** How does a Type 141/142 model-curve pointer select a neutral edge when multiple edges use the same curve carrier?

**Known.** `trimming.rs:207-212` builds `edges_by_curve` with `or_insert`, and `trimming.rs:564-565` uses that first edge. `brep.rs:858-863`, `composite.rs:426-430` and `535-540`, `offsets.rs:222-226`, and `csg.rs:30-35` also choose the first matching edge. `Edge` permits repeated `CurveId` values with distinct vertices and parameter ranges. Type 141/142 carry curve entity pointers, not edge occurrence identities.

**Need.** We need an ownership invariant or a source rule that makes the curve-to-edge relation unique, or a resolution path that verifies each candidate's range and endpoints. A wrong choice transfers the wrong range or endpoints to a boundary, B-rep, offset, composite, or sweep.

**Note.** New item filed after the hostile sweep on 2026-08-10. With two edge occurrences sharing one curve but having different spans, storage order decides; no candidate comparison or rejection exists.

## 6. Product structure, annotation, and presentation

### PS-01. Parameter defaults are honored at selected token indices only

**Question.** Which parameter fields may be omitted, and what defaults do they receive?

**Known.** `drawing.rs` applies defaults at selected token indices for several annotation and drawing records. The current defaults are documented in the codec but are not all derived from a source table.

**Need.** We need the optionality and default for each affected field, with omitted and malformed tokens distinguished.

**Note.** Closure audit 2026-08-10: reopened. Commit `c486ba66d` made the selected defaults explicit and added fixtures, but it did not establish the complete field table from the IGES specification.

### PS-02. The same text-box metric has two different bounds

**Question.** What bounds apply to the Type 212/213 text-box metrics?

**Known.** `drawing.rs` applies distinct bounds to the two record forms, including nonnegative checks for Type 312 dimensions. The current documentation records these bounds.

**Need.** We need the field definitions and bounds for each form from the IGES specification or exporter-authored witness files.

**Note.** Closure audit 2026-08-10: reopened. Commit `8d2479c8b` changed the checks and the documentation together; the test fixtures do not establish that the bounds are format rules.

### PS-04. Enumerated value tables exist only in the source

**Question.** What are the complete enumerated tables for the supported drawing and presentation fields?

**Known.** `drawing.rs` and `presentation.rs` contain the accepted values. The current tests exercise selected values and the documentation repeats the implementation tables.

**Need.** We need the enumerated tables from the format source, including reserved and invalid values, and a rule for values outside each table.

**Note.** Closure audit 2026-08-10: reopened. Commit `4c91d071e` made the source tables explicit but did not cite the specification's tables.

### PS-05. Type 420 accepts a wrong-typed type flag and Type 320 does not

**Question.** Does the Type 420 type flag have a default, and may a non-integer token satisfy it?

**Known.** `structure.rs:1959` accepts a missing or non-integer token through `is_none_or`, while the corresponding Type 320 field rejects it. The documentation does not settle defaulting or token type.

**Need.** We need the default, token type, and allowed values for both fields, with one consistent malformed-token policy.

**Note.** Closure audit 2026-08-10: reopened. Commit `d11d59213` reconciled local behavior but did not establish the format rule.

### PS-06. Type 402 Form 5 requires a non-null leader pointer

**Question.** May a Type 402 Form 5 label placement have no leader?

**Known.** `structure.rs:573-594` requires a non-null leader pointer, while other nullable pointers accept zero explicitly. The current documentation states the leader requirement.

**Need.** We need the nullability of the Form 5 leader field from the IGES specification or exporter-authored witness files.

**Note.** Closure audit 2026-08-10: reopened. Commit `45cddb592` added the requirement and fixture coverage, but did not establish it from the specification or a witness file.

### PS-07. Type 406 Form 33 requires a file-global unique identity

**Question.** Must a Type 406 Form 33 sheet identifier be unique across the file?

**Known.** `structure.rs:1033-1045` rejects duplicate `(number, name)` pairs across the file. A separate path enforces at most one identifier per drawing owner. The current documentation states both policies.

**Need.** We need the identity scope and duplicate behavior from the IGES specification or exporter-authored witness files.

**Note.** Closure audit 2026-08-10: reopened. Commit `9d0164b00` added a file-global uniqueness rule and self-authored duplicates, but did not establish that scope.

### PS-08. Type 406 Form 6 requires an ordered layer pair

**Question.** Must the Type 406 Form 6 layer numbers be in ascending order?

**Known.** `structure.rs:320-324` requires `upper >= lower`. The current documentation calls this an ordered pair.

**Need.** We need the field definitions and order rule for the Form 6 layer pair.

**Note.** Closure audit 2026-08-10: reopened. Commit `7d7a4c288` added the check and fixture, but did not establish the ordering requirement from the specification.

## 7. Write path

### WR-01. An unclassified loop is written as an inner loop

**Question.** How does the writer encode a loop whose source does not classify it as outer or inner?

**Known.** `writer.rs:2321-2364` orders loop roles and returns the first loop that is not `Inner`. The Type 510 path uses `face_outer_loop` at `writer.rs:1375-1382`; the Type 144 path also promotes the first unclassified loop. `LoopBoundaryRole::Unspecified` is a real IR state.

**Need.** We need the encoding for an unclassified loop, or a refusal when no valid outer-loop representation exists. The choice must not depend on list order.

**Note.** Closure audit 2026-08-10: reopened. Commit `ed211eb05` documented the first-unclassified policy and added generated fixtures, but it cited neither the specification nor a witness file. Two unclassified loops with different containment or orientation can produce the wrong outer boundary.

### WR-02. The declared Global minimum resolution is tighter than the writer's own acceptance bound

**Question.** What minimum resolution must a generated file declare?

**Known.** `writer.rs:3232-3245` derives a generated value from model tolerances and a floor, while `writer.rs:3037-3040` accepts some pcurve gaps against a larger effective floor. The current documentation presents the generated resolution as the settled writer policy.

**Need.** We need the declared resolution derived from the tolerances the writer accepts, and a round-trip test that proves the reader and writer use compatible bounds.

**Note.** Closure audit 2026-08-10: reopened. Commit `17c19bdcb` changed the generated-resolution policy and fixtures together. The code needs an evidence-backed relation between accepted gaps and the declared value.

### WR-03. The Type 186 outer shell is the first shell by position

**Question.** Which shell of a region is the exterior shell?

**Known.** `cadmpeg-ir/src/topology.rs:97-110` documents ordered shells and `Region::exterior_shell()` returns `self.shells.first()`. `writer.rs:1442-1457` uses that accessor for Type 186. No validation proves containment, orientation, or producer ordering.

**Need.** We need the exterior shell identified from geometry, an explicit source role, or a validated IR invariant. List position alone must not invert a solid.

**Note.** Closure audit 2026-08-10: reopened. Commit `46a71f68c` introduced the IR wording, accessor, writer use, and tests together. This is promotion to an IR invariant, not evidence that all producers supply the order.

### WR-04. Global fields are a fixed string

**Question.** Which Global values must a generated file compute from the model?

**Known.** `writer.rs:4822-4835` writes fixed sender, file, author, organization, timestamp, resolution, and coordinate metadata. The current documentation records this as writer policy.

**Need.** We need the fields that must be computed, the fields that may use project defaults, and the required treatment of timestamps and source identity.

**Note.** Closure audit 2026-08-10: reopened. Commit `2b3306edc` made the fixed metadata explicit, but no interoperability evidence or settled project policy justifies the values.

### WR-05. The target version changes one digit only

**Question.** What does `IgesWriteOptions::version` constrain?

**Known.** `writer.rs:295-310` rejects one unsupported Type 514 form for older targets, while `version.global_flag()` changes the Global version field. Other emitted entity families are not checked against the selected target version.

**Need.** We need the entity and form set of each target version, and a refusal when the model requires an entity the target does not define.

**Note.** Closure audit 2026-08-10: reopened. Commit `1a6b988e7` added a target entity check, but its coverage and version matrix are not independently established.

### WR-06. The analytic surface family is fixed with no fallback

**Question.** Which IGES surface entity should a generated file use for each analytic surface?

**Known.** `writer.rs` maps planes, cylinders, cones, spheres, and tori to Types 190, 192, 194, 196, and 198, and rejects unsupported native forms. The writer does not record why this family is preferred over Type 108/120/128 alternatives.

**Need.** We need the encoding choice and its interoperability evidence, plus a loss or refusal when the selected family is not supported by the target profile.

**Note.** Closure audit 2026-08-10: reopened. Commit `f4a07d64b` made the analytic-family choice explicit but did not establish portability or a target-profile rule.

### WR-07. Orthonormality gates refuse foreign frames instead of repairing them

**Question.** What frame perturbation must the writer accept?

**Known.** `writer.rs` uses `FRAME_REPAIR_DOT_LIMIT = 1e-6` in `orthonormal_pair`; the current writer repairs only within that project-selected bound and rejects larger residuals.

**Need.** We need a source or producer-derived bound for representational frame noise, and a rule for repair versus refusal.

**Note.** Closure audit 2026-08-10: reopened. Commit `dc2bd137a` added the repair threshold and synthetic boundary tests, but the bound is not established from the specification or witness files.

### WR-10. Fixed protocol constants with no IR source

**Question.** What are the correct `PREF`, creation-method, and hierarchy values for generated records?

**Known.** `writer.rs` emits fixed values for Type 141/142 preferences, Type 142 creation method, and Type 504 hierarchy. The values are justified by the current neutral IR and writer behavior, not by a complete source mapping.

**Need.** We need the correct value for each field and independent evidence for the Type 504 hierarchy difference.

**Note.** Closure audit 2026-08-10: reopened. Commit `82c13da5a` recorded protocol constants as settled without external format or producer evidence.

## 8. Evidence

### EV-01. No file authored by another system has ever been decoded

**Question.** Does the decoder read IGES files that this project did not write?

**Known.** `corpus/manifest.toml` holds eleven files and every one is `fcstd`. No IGES file exists in the corpus. `iges-fixture-charter.md` states that fixtures are generated from builders that "serialize the rules in `iges.md`" and "do not ingest, rewrite, minimize, or transform external files".

**Conflict.** The decoder is tested only against bytes written by this project's own reading of the format. Agreement between a builder and a decoder that share one author proves that the two agree. It does not test the reading.

**Note.** Nearly every item in sections 1 through 6 above is a rule that a self-authored fixture satisfies by construction and that a producer file may not. The items were found by reading, not by testing, because no test could have found them.

**Need.** We need IGES files authored with at least two available exporters, under a license the repository admits, decoded and recorded. That measurement decides which of the tolerance items above are real and which are theoretical.

### EV-02. The independent-application gate cannot detect wrong geometry

**Question.** What does FreeCAD acceptance prove about a generated file?

**Known.** `scripts/verify-iges-freecad.py` imports each file and refuses an import that gives no object or whose shapes are null or invalid (`:37-50`). It counts solids and faces and asserts nothing about them. A file with the wrong units, a mirrored surface, an inverted solid (WR-03), or an unbounded face (WR-01) imports as a valid shape and passes.

**Note.** The script is wired into no CI job and no test, and it needs a manual environment. No result artifact is committed, so no run is on record. The script globs `*.igs` only (`:68`). The CLI accepts and writes both `.igs` and `.iges` (`crates/cadmpeg/src/main.rs:168`), so a directory of `.iges` output is silently outside the check.

**Need.** The P0 gate above requires independent native-application acceptance. We need the acceptance criterion to compare geometry with the intended model, the glob to cover both extensions, and each run recorded.

### EV-03. The fixture builders and the decoder share one author

**Question.** Which decoder rules do the fixtures actually test?

**Known.** `iges-fixture-charter.md` states that builders serialize the rules in `iges.md`. A builder therefore writes the byte pattern that the decoder expects. Where a decoder rule is a guess, the builder embodies the same guess, and the test passes for both.

**Note.** GE-01 is the demonstrated case. Commit `f20d17e65` set `TRANSFORM_TOLERANCE` and authored the fixture that justifies it in the same commit, perturbed to 5e-11 against a threshold of 1e-10.

**Need.** We need each tolerance and default in sections 2 through 5 traced to evidence outside this repository, or marked as a project convention in `iges.md` rather than as a format rule.
