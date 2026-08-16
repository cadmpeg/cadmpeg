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
