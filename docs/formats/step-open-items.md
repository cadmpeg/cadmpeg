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

### TP-09. Pcurve endpoint and tied-locus verification

**Question.** What evidence proves that a non-seam pcurve candidate is the
correct edge carrier, and that tied candidates have the same model-space
locus?

**Known.** `select_associated_pcurve` scores candidate endpoint fits and
accepts the lowest finite score within tolerance. A tie is
declared from a relative score threshold. Declared pcurve trims are checked at
their own endpoints; a stale trim falls back to an independently inverted edge
interval. `pcurve_loci_equivalent` includes NURBS breakpoints, performs bounded
adaptive subdivision, and selects the lowest STEP identity for equivalent ties.
Endpoint inversion uses a finite uniform grid plus NURBS knot boundaries and
their span midpoints, followed by bounded iterative closest-point calculation.

**Note.** TP-02 records the semantic selection rule, but this implementation
does not prove a global minimum or a global locus equivalence. A pcurve with
an endpoint minimum outside the finite seed grid can still be missed. Adaptive
subdivision returns unresolved when its depth limit cannot establish a flat
interval, but it is not an interval-arithmetic proof for arbitrary surface and
pcurve compositions. This item records the verification gap rather than
treating the numerical heuristic as STEP semantics.

**Need.** We need multi-pcurve witness files, authored with an available
exporter or taken from a public corpus, and an exact inverse or
interval/adaptive proof for endpoint fit and locus equivalence, including
reordered, near-tied, and crossing candidates.

## 6. Units and measures

## 7. Annotation, presentation, and tessellation

## 8. QA-reopened closure items
