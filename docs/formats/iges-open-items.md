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

### PH-03 — Generic trailing pointer boundary selection

**Question.** When a Parameter Data record has more than one structurally valid
trailing pointer-group boundary, what rule identifies the primary-parameter end?

**Known.** `parameter.rs` enumerates structural candidates in token order and
returns the first target-valid candidate. `native.rs` uses that boundary as the
primary-parameter end. `ParameterBoundaryAmbiguous` is emitted only for a
required back-pointer route when no candidate is selected. The test
`earliest_valid_trailing_pointer_group_boundary_wins` requires the earliest
valid candidate.

**Need.** A variable-length primary list must not be cut at a pointer-shaped
suffix merely because that suffix validates against the Directory. The decoder
must have an entity-specific discriminator, or retain the alternatives and
report ambiguity before assigning parameter and pointer ownership.

**Conflict.** IGES 5.3 §2.2.4.5.2 places association and property groups after
the primary parameters. It does not state that the earliest target-valid
structural candidate is the universal boundary rule.

**Note.** Commit `11321f89cdfb` closed this item by promoting the earliest-valid
candidate to the specification. A variable-length entity can contain a valid
pointer-shaped suffix before the actual trailing groups. The ordinary path then
cuts the primary fields, assigns them to association or property groups, and
emits no ambiguity loss. This is a partial closure.

## 2. Global metadata

## 3. Directory fields, the reference graph, and the native arenas

### DR-01 — Display-attribute pointer resolution

**Question.** When a Directory display field is a negative pointer, when may
the native display arena expose a typed definition identity?

**Known.** `native.rs` creates a `NativeDisplayAttributes` record for every
Directory entry and constructs `line_font_definition`, `level_definition`, and
`color_definition` identities directly from the signed fields. `graph.rs`
separately resolves the exact target types and emits `graph.pointer-unresolved`
for a missing or wrong target. The direct native projection does not use that
resolution result.

**Need.** A typed native identity must resolve to an existing definition of the
required type and form, or the field must remain raw and carry an attributed
loss. A consumer must not receive a dead typed link.

**Conflict.** The native arena can expose `iges:presentation:*#D<n>` for a
missing or wrong target while the reference graph says that the pointer is
unresolved. Native raw retention does not validate the typed identity.

**Note.** A Directory entry with `line_font = -99` and no Type 304 D99
produces a non-null typed native path with no corresponding line-font record.
The same failure exists for level and color. The broad closure in
`6f8556118971` did not cover this direct projection path. This is a partial
closure.

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path
