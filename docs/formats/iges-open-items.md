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

**Known.** IGES 5.3 §§2.2.4.3.14–2.2.4.3.15 make field 14 authoritative except for flag `3`, list the ordinary flag-15 payloads, and delegate a flag-3 name to MIL-STD-12 or IEEE 260. Type 316 is a property-pointer attachment whose scale applies to the real data of its owning entity. The decoder accepts a nonempty flag-3 string, retains it for inspection, and refuses semantic projection when its length factor is unknown.

**Need.** We need to establish whether the delegated flag-3 standards define a closed alias and factor contract, or whether the IGES contract ends at the delegated name.

**Conflict.** The earlier exact-eleven-name rule conflicts with the flag-3 delegation and with Open CASCADE's documented user-defined flag-3 name. The codec must not silently rescale geometry without a proven factor.

**Note.** This pass has settled field-14 precedence, the ordinary flag table, nonempty flag-3 storage, Type 316 scope, and the semantic refusal boundary. The external namespace and factor boundary remain under test.

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path
