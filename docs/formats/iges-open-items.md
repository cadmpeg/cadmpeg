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

### GE-03. Type 112 segment continuity

**Question.** Which Type 112 continuity constraints are format requirements for `H=0`, `H=1`, and `H=2`?

**Known.** `entities/splines.rs:430-489` checks position, unit-tangent, and curvature interval overlap according to the declared continuity class. The current specification states the same constraints without a direct IGES section citation. The closure change added implementation and synthetic cases only.

**Need.** Cite the Type 112 continuity table and define the required receiver action for a failed redundant endpoint or derivative field.

**Conflict.** The specification treats the continuity tests as format-authoritative, but the closure evidence does not include a primary-source or independent-producer witness.

**Note.** Reopened by QA audit 2026-08-16. Existing tests exercise the implementation decision and cannot establish the source semantics.

### GE-05. Type 102 carrier concatenation uses a private tolerance and degrades silently

**Question.** What endpoint agreement rule governs Type 102 child concatenation?

**Known.** `entities/composite.rs:135-196` selects candidate edges with a supplied join tolerance, and `entities/composite.rs:758-797` marks joins continuous only when that tolerance succeeds. The current specification requires strict Global minimum-resolution agreement and records a geometry loss on failure, but it has no direct IGES section citation. The closure change altered the implementation, tests, and prose together.

**Need.** Cite the Type 102 join rule or classify the use of Global minimum resolution and the degradation path as CADIR policy. A failed exact carrier must remain visible with the defined loss.

**Conflict.** The specification claims a format-authoritative endpoint rule while the closure evidence does not independently establish it.

**Note.** Reopened by QA audit 2026-08-16. The existing synthetic joins do not establish the source threshold or receiver behavior.

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

### PS-01. Parameter defaults are honored at selected token indices only

**Question.** Which Type 410, Type 212, and Type 213 fields are optional, and what defaults do they receive?

**Known.** `entities/annotation.rs:51-165` and `entities/drawing.rs:13-23` apply defaults with `integer_or`, `number_or`, and `string_or_empty`. The current specification states the current defaults, but it gives no direct IGES field-table citation for these records. The closure change supplied code and synthetic tests, not independent field evidence.

**Need.** Trace each optional field and default to its IGES entity definition. Distinguish an omitted field from a malformed supplied field.

**Conflict.** The specification presents the defaults as format rules while the cited source coverage does not identify the Type 410, Type 212, or Type 213 field tables.

**Note.** Reopened by QA audit 2026-08-16. The current code and tests cannot establish source optionality.

### PS-04. Enumerated value tables exist only in the source

**Question.** What are the complete enumerated tables for the supported view, annotation, leader, and presentation fields?

**Known.** `entities/annotation.rs:35-49` and `entities/drawing.rs:41-55` implement local tables for justification, mirror, orientation, depth, display, line-font, and color values. The current specification repeats selected ranges and pattern codes, but it gives no direct IGES field-table citations for the complete presentation set.

**Need.** Cite the complete source tables, including reserved and invalid values, and define the handling of values outside each table.

**Conflict.** The specification presents local validation tables as settled format rules while the closure evidence does not independently establish the complete tables.

**Note.** Reopened by QA audit 2026-08-16. Code, prose, and synthesized fixtures are one implementation evidence chain.

## 7. Write path
