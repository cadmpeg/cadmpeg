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

**Question.** Which entity-specific rule supplies `NV`, the last primary Parameter Data index, when a generic scan finds multiple structurally closed and target-valid suffixes?

**Known.** IGES 5.3 §2.2.4.5.2 places the two trailing pointer groups after all specified or defaulted entity parameters and defines `NV` as the last parameter number. The entity tables define the primary indexes; Type 116 §4.16, for example, lists indexes 1 through 4 before the additional pointer groups. `parameter.rs:261-283` scans every token position and accepts a suffix only when one candidate is fully target-valid. `native.rs:1497-1502` uses that result for `primary_end`; `native.rs:1525-1541` records a loss and leaves all tokens primary when several candidates are valid. The code-built ambiguity witness in `parameter/tests.rs:385-463` checks that refusal and raw-token retention.

**Need.** We need an entity-specific `NV` rule, or an explicit fallback boundary for entity forms without a usable layout. Without it, a valid pointer group can become primary data, or a valid relationship can remain unassigned, when the candidate scan finds more than one target-valid suffix.

**Conflict.** The Parameter Data, counted-parameter, and Entity graph sections in `iges.md` state that the entity-specific boundary precedes the trailing groups and record unique-candidate recovery as the rule, but the generic decoder does not consult the entity type and form layouts or an `NV` table. The source defines `NV` after the entity layout is known; the current closure treats the absence of a generic tie-breaker as the format answer.

**Note.** The ambiguity witness proves that the conservative fallback is observable. It does not prove that a conformant entity lacks the entity-specific `NV` boundary.

### PH-08. Pre-Terminate unsequenced physical records

**Question.** Must a Fixed ASCII physical line that is unsequenced because its section marker is absent or invalid be rejected before the Terminate Section?

**Known.** IGES 5.3 §2.2 states that the file consists of 80-column lines with a section code in column 73 and an ascending sequence in columns 74 through 80, and that unsequenced lines shall not appear before Terminate. `card.rs:178-183` maps an unrecognized section marker to `None`; a recognized marker with a bad sequence is checked separately. `card.rs:215-225` skips every line with no section while validating order, and `card.rs:326-329` then accepts the scan. Global, Directory, and Parameter readers filter by recognized sections (`global.rs:166-170`, `directory.rs:154-168`, and `parameter.rs:568-573`). The Physical representation section in `iges.md` allows unsequenced lines only after Terminate.

**Need.** We need the decoder to reject a pre-Terminate unsequenced line, or to state and account for a defined recovery that preserves its semantics. Section counts, sequence validation, and semantic projection must not ignore a physical record that the format forbids.

**Conflict.** A blank 80-byte line or a line with an unrecognized marker inserted between valid pre-Terminate cards is accepted by `validate_card_order`; it is omitted from every parsed section and has no loss note. `card.rs:412-432` reports it only in the inspection summary as an opaque noncanonical record, while decode retains it only as an unclassified native card. The decoder therefore admits a file that the IGES physical framing rule forbids and silently excludes the line from section data.

## 2. Global metadata

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path
