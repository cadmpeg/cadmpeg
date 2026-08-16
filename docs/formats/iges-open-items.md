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

### GE-09. Type 104 endpoints are not independently authoritative

**Question.** Must Type 104 endpoint coordinates agree with the conic parameters, and what tolerance applies?

**Known.** IGES 5.3 §4.5 says that the six coefficients and the ordered start and terminate points define the conic arc. Section 2.2.4.3.19 says that a receiving system considers coordinate locations less than the Global minimum resolution apart coincident. `entities/conics.rs:385-414` evaluates the coefficient-defined carrier, rejects an endpoint at or beyond the resolution, and retains the declared endpoint coordinate when it accepts the entity. The Geometry section of `iges.md` presents this rejection and vertex-retention policy as a format rule.

**Need.** Separate the format rule from the CADIR admission decision. The primary text must establish whether endpoint disagreement makes a Type 104 entity invalid and whether the strict Global-resolution comparison is required, or the specification must mark those receiver choices as CADIR decisions.

**Note.** The 2026-08-16 closure audit reopened GE-09. Commit `1c014ac71` cited §4.5 and §2.2.4.3.19 and used an authored differential witness. The primary text establishes both endpoint and coefficient fields and the general coincidence resolution, but it does not state the endpoint-evaluation rejection algorithm. The witness verifies the implementation, not an independent format requirement.

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path
