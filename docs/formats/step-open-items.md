# STEP Open Items

This document lists the parts of STEP exchange formats that we do not know. The specification `step.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. External resources

## 3. Containers and other encodings

### CE-06. Part 26 graph binding

**Question.** How does Part 26 data bind to a Part 21 graph when both resources describe one exchange?

**Known.** Part 26 and Part 21 use separate encodings and require schema-specific identity and mapping rules. The codec does not compose them.

**Need.** Define the resource identity, graph-binding operands, conflict policy, and retention rules for a composed Part 26 and Part 21 result.

**Conflict.** The decoder refuses Part 26 and never compares or binds its graph to Part 21. The current caller-boundary statement does not supply a composition witness.

**Note.** `crates/cadmpeg-codec-step/src/codec.rs:365-370` refuses HDF5 before graph construction. `step.md` §1 "CADIR decision: the STEP codec classifies an HDF5 signature at an allowed" explicitly leaves graph binding to the caller. No witness proves that identities, references, units, or conflicts remain resource-scoped during composition.

## 4. Signatures

## 5. Topology and pcurve decisions

## 6. Units and measures

## 7. Annotation, presentation, and tessellation

## 8. Product structure and placement
