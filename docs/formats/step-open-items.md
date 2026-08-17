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

## 4. Signatures

## 5. Topology and pcurve decisions

## 6. Units and measures

## 7. Annotation, presentation, and tessellation

## 8. Product structure and placement

### BM-02. BO-Model composition

**Question.** How do BO-Model XML identities and values combine with a Part 21 instance graph?

**Known.** The encodings have separate identity systems and explicit external-file references (`step.md` §1 "Part 21 does not define a sidecar filename", `step.md` §1 "The BO-Model XML identity system is local"). The codec refuses BO-Model XML (`crates/cadmpeg-codec-step/src/codec.rs:365-379`).

**Need.** Define an AP242 cross-file identity relation, precedence policy, and independently paired XML and Part 21 exchanges.

**Conflict.** Refusing BO-Model XML avoids an accidental join but does not implement or settle cross-file composition.
