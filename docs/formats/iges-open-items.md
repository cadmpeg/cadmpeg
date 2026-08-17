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

### PH-03. Entity-specific boundary for trailing pointer groups

**Question.** Which remaining supported entity-specific layouts must supply `NV`, the last primary Parameter Data index, before generic recovery is allowed?

**Known.** IGES 5.3 §2.2.4.5.2 places the two trailing pointer groups after all specified or defaulted entity parameters and defines `NV` as the last parameter number. The entity tables define the primary indexes. Type 123 Form 0 §4.20 lists X, Y, and Z at indexes 1 through 3; `parameter.rs::entity_primary_end` now selects token index 4 for that form before generic scanning. A synthetic Type 123/Form 7 witness uses Type 123 tokens `123,0,0,2,1,1,0;` and Type 402 Form 7 `402,1,3;`: the Type 123 table boundary is token 4, while generic scanning also accepts token 3. The rebuilt decoder assigns the Type 402 association link and emits no boundary-ambiguous loss. `native.rs:1497-1502` uses the selected result for `primary_end`; `native.rs:1525-1541` reports ambiguity only for generic layouts.

**Need.** We need to trace and register the primary layout for every remaining supported variable-width entity form. Without it, a valid pointer group can become primary data, or a valid relationship can remain unassigned, when the candidate scan finds more than one target-valid suffix.

**Conflict.** The Parameter Data, counted-parameter, and Entity graph sections in `iges.md` now state that a proven entity-table boundary takes precedence and that unique-candidate recovery is only a CADIR fallback. The decoder implements this precedence for Type 123 Form 0; the remaining supported layouts still use the generic fallback until their table rules are proven.

**Note.** The earlier Type 116 ambiguity fixture was insufficient because it did not establish a conformant Type 116 primary layout plus a valid relationship group. The Type 123/Form 7 witness supplies the different evidence required by the audit and settles Type 123 only; do not delete this item until the remaining supported layouts are covered.

## 2. Global metadata

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path
