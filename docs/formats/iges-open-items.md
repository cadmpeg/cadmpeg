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

### PH-04. A physical line longer than 80 bytes

**Question.** How does a physical line with more than 80 payload bytes divide into records?

**Known.** IGES 5.3 §§2.2 and 2.2.4.6 define 80-column Fixed ASCII cards and permit unsequenced data after the Terminate section. `card.rs:162-208` accepts an overlong line when its first 80-byte header is `T`, emits the card, and retains the remainder as an unsequenced physical record. `card/tests.rs:171-187` asserts this behavior. The Physical representation section of `iges.md` records the same split.

**Need.** Establish whether the post-Terminate remainder may share the physical line that contains the Terminate card, or must be a following unsequenced line. Classify the split as an IGES rule or as a CADIR recovery rule.

**Note.** The 2026-08-16 closure audit reopened PH-04. Commit `11321f89c` used the fixed-line and post-Terminate clauses plus a synthesized witness, but those clauses do not state the same-line division explicitly. The next probe must use the primary wording and an exporter-authored witness, then mark any receiver recovery policy as CADIR in the specification.

## 2. Global metadata

## 3. Directory fields, the reference graph, and the native arenas

### DR-17. Native counted fields are not bounded before trailing pointer groups

**Question.** Must every native counted field stop at the selected entity-specific boundary before association and property pointer groups?

**Known.** `native.rs:1475-1484` computes `primary_end` from the selected trailing-group boundary. `ParameterDataRecord::count` instead delegates to `count_with_stride` with `self.tokens.len()` at `parameter.rs:140-150`. Type 406 Form 1 uses the unbounded accessor at `native.rs:1914-1919`, although IGES 5.3 §4.98 places its `NP` level list before “Additional pointers as required”. The sweep also found unbounded count readers at `native.rs:1492-1494`, `1767-1769`, `2047-2049`, `2255-2258`, `2538-2541`, `2581-2584`, `2659-2662`, `2759-2762`, `2812-2815`, `2837-2840`, `2849-2852`, `2870-2873`, `2898-2901`, `3088-3091`, `3250-3275`, `3686-3689`, `3799-3805`, `3887-3890`, `3955-3963`, `4103-4119`, `4203-4206`, `4366-4369`, `4460-4467`, `4498-4501`, and `4557-4560`.

**Need.** Complete the entity-form audit and make each count-driven native list use the entity-specific end. A deliberately oversized count must not consume valid trailing pointer-group tokens as typed list values. Raw tokens and a source-attributed malformed state must remain available.

**Note.** New finding from the 2026-08-16 hostile boundary sweep. For Type 406 Form 1, an overstated `NP` can fit only because the full token stream includes the trailing pointer-group count and pointers; `record.count(1)` then exposes those tokens as level values. The specification already states the pre-group boundary rule, so this is a code/spec mismatch, not an unsettled format meaning.

## 4. Geometry carriers and tolerances

### GE-09. Type 104 endpoints are not independently authoritative

**Question.** Must Type 104 endpoint coordinates agree with the conic parameters, and what tolerance applies?

**Known.** IGES 5.3 §4.5 says that the six coefficients and the ordered start and terminate points define the conic arc. Section 2.2.4.3.19 says that a receiving system considers coordinate locations less than the Global minimum resolution apart coincident. `entities/conics.rs:385-414` evaluates the coefficient-defined carrier, rejects an endpoint at or beyond the resolution, and retains the declared endpoint coordinate when it accepts the entity. The Geometry section of `iges.md` presents this rejection and vertex-retention policy as a format rule.

**Need.** Separate the format rule from the CADIR admission decision. The primary text must establish whether endpoint disagreement makes a Type 104 entity invalid and whether the strict Global-resolution comparison is required, or the specification must mark those receiver choices as CADIR decisions.

**Note.** The 2026-08-16 closure audit reopened GE-09. Commit `1c014ac71` cited §4.5 and §2.2.4.3.19 and used an authored differential witness. The primary text establishes both endpoint and coefficient fields and the general coincidence resolution, but it does not state the endpoint-evaluation rejection algorithm. The witness verifies the implementation, not an independent format requirement.

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path
