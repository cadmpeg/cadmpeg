# IGES open items

IGES L9 is not achieved. The current score is L8 tested. The bounded semantic
writer and its independent-application checks are extras above L8; they do not
close the L9 gate while decode can time out, return invalid `CadIr`, or omit
semantic records from transfer.

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

The decoder can return success for documents that `cadmpeg validate` rejects.
Observed failures include edge parameter ranges outside their canonical curve
domains and edge curve endpoints that do not meet their vertex positions.

Required closure:

- canonicalize or reject carrier domains before committing edges;
- validate edge endpoints, pcurves, topology ownership, and transforms before
  returning decode success;
- commit no partial topology after a failed validation; and
- add synthesized fixtures for each failure class and run decode followed by
  `cadmpeg validate` in the regression gate.

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
- make `--strict` reject all losses that can change model, topology, product,
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

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path

## 8. Evidence

### EV-01. No file authored by another system has ever been decoded

**Question.** Does the decoder read IGES files that this project did not write?

**Known.** `corpus/manifest.toml` holds eleven files and every one is `fcstd`. No IGES file exists in the corpus. `iges-fixture-charter.md` states that fixtures are generated from builders that "serialize the rules in `iges.md`" and "do not ingest, rewrite, minimize, or transform external files".

**Conflict.** The decoder is tested only against bytes written by this project's own reading of the format. Agreement between a builder and a decoder that share one author proves that the two agree. It does not test the reading.

**Note.** Nearly every item in sections 1 through 6 above is a rule that a self-authored fixture satisfies by construction and that a producer file may not. The items were found by reading, not by testing, because no test could have found them.

**Need.** We need IGES files from at least two independent producers, under a license the repository admits, decoded and recorded. That measurement decides which of the tolerance items above are real and which are theoretical.

### EV-02. The independent-application gate cannot detect wrong geometry

**Question.** What does FreeCAD acceptance prove about a generated file?

**Known.** `scripts/verify-iges-freecad.py` imports each file and refuses an import that gives no object or whose shapes are null or invalid (`:37-50`). It counts solids and faces and asserts nothing about them. A file with the wrong units, a mirrored surface, an inverted solid (WR-03), or an unbounded face (WR-01) imports as a valid shape and passes.

**Note.** The script is wired into no CI job and no test, and it needs a manual environment. No result artifact is committed, so no run is on record.

**Note.** The script globs `*.igs` only (`:68`). The CLI accepts and writes both `.igs` and `.iges` (`crates/cadmpeg/src/main.rs:168`), so a directory of `.iges` output is silently outside the check.

**Need.** The P0 gate above requires independent native-application acceptance. We need the acceptance criterion to compare geometry with the intended model, the glob to cover both extensions, and each run recorded.

### EV-03. The fixture builders and the decoder share one author

**Question.** Which decoder rules do the fixtures actually test?

**Known.** `iges-fixture-charter.md` states that builders serialize the rules in `iges.md`. A builder therefore writes the byte pattern that the decoder expects. Where a decoder rule is a guess, the builder embodies the same guess, and the test passes for both.

**Note.** GE-01 is the demonstrated case. Commit `f20d17e65` set `TRANSFORM_TOLERANCE` and authored the fixture that justifies it in the same commit, perturbed to 5e-11 against a threshold of 1e-10.

**Need.** We need each tolerance and default in sections 2 through 5 traced to evidence outside this repository, or marked as a project convention in `iges.md` rather than as a format rule.

### TE-01a. Integration arena rows do not name the subject entity's arena

**Question.** Which arena must each integration fixture populate, and which entity in that fixture must populate it?

**Known.** TE-01 asked for a per-fixture expectation table. Commit `0352402f3` delivered one: `integration_tests.rs:47-62` names an arena per fixture and fails with the fixture name. The union assertions are gone. The remaining defects are narrower than TE-01 and are recorded here:

- Every row asserts `arena_count(...) > 0`. No row declares a count.
- Some rows name an arena that a different entity in the same fixture fills. `mixed_analytic_composite_curve` (`tests.rs:854-880`) holds a Type 100, a Type 110, and a Type 102, and maps to `ModelCurves` (`integration_tests.rs:120-124`). The matrix gives Type 102 the destination `model.procedural_curves`, and Types 100 and 110 fill `model.curves` on their own. The row passes when the Type 102 decoder emits nothing. The same shape applies to `procedural_and_boolean_solids` and to five drawing fixtures that all name `Native("annotations")` while each holds a Type 212 that satisfies the row alone.
- The table names the decoder's internal arena keys and is not cross-checked against the `destination` column of `corpus/iges-envelope-a.toml`. Envelope admission is cross-checked against that file (`tests.rs:190-242`); the arena table is not.

**Need.** We need each row to name the arena of the entity the fixture exists to exercise, and a test that compares the table with the matrix `destination` column.

### EV-07. Tolerance gates are bracketed at 100 times the threshold

**Question.** Which tolerance value does each gated test actually pin?

**Known.** Every fixture declares a Global minimum resolution of `0.001`. The accept and reject pair for the Type 100 radius gate uses a radius delta of 1.7e-9 to accept and 9.8e-2 to refuse (`tests.rs:8298`, `:8325`). The threshold sits between them with seven orders of magnitude of slack, so any tolerance in `[2e-9, 9.7e-2)` keeps both tests green, including removal of `minimum_resolution_mm()` from `geometry.rs:380`. The three carrier-disagreement reject tests use a 0.1 coordinate shift against the same 0.001 tolerance.

**Note.** `bounded_plane_with_resolution_gap_file` (`tests.rs:2464`) is the exception and shows the correct shape: a 0.0005 gap against a 0.001 tolerance. It has no reject-side twin.

**Need.** GE-01 through GE-10 record tolerances that no test pins. We need each gate bracketed on both sides.
