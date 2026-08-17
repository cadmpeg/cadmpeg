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

### GL-02. Delegated Global flag-3 unit names

**Question.** Which flag-3 unit symbols conform to the delegated standard, and which admitted symbol-to-millimetre factors may semantic projection use?

**Known.** `global.rs:424-433` admits every nonempty flag-3 Hollerith string. `global.rs:501-519` then reuses the ordinary exact alias table to decide whether a flag-3 name has a known length factor. `reader.rs:113-116` refuses semantic decode only when that table has no factor. The Global units paragraphs in `iges.md` state that the delegated standard controls the symbol form and that the ordinary table is not used for flag 3.

**Need.** We need the delegated symbol admission and factor table from [IGES 5.3 §§2.2.4.3.14–2.2.4.3.15](https://paulbourke.net/dataformats/iges/IGES.pdf), [MIL-STD-12D §4.7](https://www.expresscorp.com/wp-content/uploads/2023/02/MIL-STD-12D.pdf), and [IEEE 260-1978](https://standards.ieee.org/ieee/260/440). The decoder must either validate that contract or define an opaque-name boundary without treating ordinary aliases as delegated symbols.

**Conflict.** The Global units specification says that flag-3 symbol form is delegated and is not compared with the ordinary table. The decoder admits arbitrary nonempty names, but recognizes flag-3 conversion factors by the ordinary table. The same source name can therefore be admitted as a delegated symbol while its conversion status depends on an unrelated alias list.

**Note.** The current `2Hmm` and `nmi` witnesses establish retention and refusal behavior only. They do not establish delegated conformance or the factor boundary.

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path
