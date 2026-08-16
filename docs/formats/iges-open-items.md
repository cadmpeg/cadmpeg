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

### PS-04. Type 213 `CHRSET` is not admitted

**Question.** Which Type 213 `CHRSET` values and Type 310 pointer targets are valid, and does semantic projection enforce them?

**Known.** [IGES 5.3 §4.61](https://paulbourke.net/dataformats/iges/IGES.pdf) defines the Type 213 character-set table: `1`, `1001`, `1002`, `1003`, `2001`, and `3001`, with a Type 310 pointer form. `entities/annotation.rs:128-145` validates the Type 213 `FONT` field at `start + 5` but never reads `CHRSET` at `start + 11`. `native.rs:4266-4310` retains that field and resolves negative Type 310 pointers, but no semantic admission check uses it. The Type 213 paragraph in `iges.md` states the table and says invalid presentation values suppress semantic projection.

**Need.** Validate explicit `CHRSET` values against the IGES table and validate Type 310 pointer targets. Apply the documented default only when the field is omitted. An invalid supplied value must retain the native record and suppress semantic projection with an attributed loss.

**Conflict.** The Type 213 paragraph in `iges.md` states that the complete presentation table is enforced and that invalid values suppress semantic projection, but `new_general_note_valid` does not inspect `CHRSET`.

**Note.** Reopened by the QA audit on 2026-08-17. A Type 213 record with every other field valid and `CHRSET = 0` or `4`, or a negative pointer to a wrong target, passes `new_general_note_valid`, projects a semantic annotation, and emits no presentation loss. The existing tests cover Type 213 `FONT`, metrics, counts, and defaults, but do not exercise `CHRSET`.

## 7. Write path
