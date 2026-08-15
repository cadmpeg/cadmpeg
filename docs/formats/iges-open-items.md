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

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path

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

**Note.** Closure audit 2026-08-10: reopened. Commit `dc2bd137a` added the repair threshold and synthetic boundary tests, but no external evidence establishes the bound.

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

**Need.** We need IGES files from at least two independent producers, under a license the repository admits, decoded and recorded. That measurement decides which of the tolerance items above are real and which are theoretical.

### EV-02. The independent-application gate cannot detect wrong geometry

**Question.** What does FreeCAD acceptance prove about a generated file?

**Known.** `scripts/prepare-iges-freecad-golden.py` materializes either the exact geometry manifest or every checked-in golden with a successful writer output. `scripts/prepare-iges-freecad-edited.py` creates one edited neutral point document for each IGES target version. `scripts/verify-iges-freecad.py` imports each file, refuses an import that gives no object unless the broad writer pass explicitly allows presentation-only emptiness, and refuses shapes that are null or invalid. The exact pass requires a complete manifest of topology counts, bounding-box coordinates, and scalar measures with explicit tolerances. `scripts/iges-freecad-expectations.json` covers finite points, curves, trimmed surfaces, and B-rep solids; unbounded analytic surfaces use topology counts only. The workflow runs both passes.

**Note.** `.github/workflows/iges-freecad.yml` runs the gate on pull requests, main pushes, scheduled runs, and manual dispatch, then uploads the report. The script still needs an external FreeCAD runtime, and no result artifact is committed to the repository.

**Need.** Require one successful workflow run for the complete writable geometry profile and retain both report artifacts as evidence. Expand the exact manifest when a geometry profile grows and keep the broad pass aligned with every successful writer output.

### EV-03. The fixture builders and the decoder share one author

**Question.** Which decoder rules do the fixtures actually test?

**Known.** `iges-fixture-charter.md` states that builders serialize the rules in `iges.md`. A builder therefore writes the byte pattern that the decoder expects. Where a decoder rule is a guess, the builder embodies the same guess, and the test passes for both.

**Note.** GE-01 is the demonstrated case. Commit `f20d17e65` set `TRANSFORM_TOLERANCE` and authored the fixture that justifies it in the same commit, perturbed to 5e-11 against a threshold of 1e-10.

**Need.** We need each tolerance and default in sections 2 through 5 traced to evidence outside this repository, or marked as a project convention in `iges.md` rather than as a format rule.

### EV-07. Tolerance coverage is partial and not independently bracketed

**Question.** Which tolerance and default decisions have independent accept/reject boundary evidence?

**Known.** The current suite has paired geometry cases for selected Type 141/142 boundaries, Type 104 endpoints, Type 100 arcs, Type 102 joins, Type 112 continuity, and Type 106 endpoints. It does not provide paired boundary cases for every tolerance or default currently stated in sections 2 through 5; the transform, angular, frame, flag, and several default policies remain unbracketed.

**Need.** We need an independent accept/reject pair for each material tolerance and default, or a project-policy classification that removes the claim of format authority.

**Note.** Closure audit 2026-08-15: reopened. The mass-closure tests improved coverage but closed this item only partially; selected geometry pairs do not establish coverage of every active gate.
