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

### TP-09. Pcurve endpoint proof boundary

**Question.** What exact proof establishes endpoint fit for the unique
non-seam pcurve candidate after source association, including a stale finite
trim or an unbounded candidate?

**Known.** ISO 10303-42:2021 §5.2.2 and §5.2.2.1 require associated pcurves to
describe the edge in model space and require parameter-space connectivity when
multiple same-surface candidates exist. The CADIR policy now leaves every
competing same-surface set detached; a single candidate uses the existing
finite seeded endpoint witness, and a declared stale trim may retain the
recovered edge interval on that coedge use. Candidate list order, STEP
identity, and mapped-locus comparison do not select a competing carrier.

**Need.** We need an independent mathematical proof or validated interval
bound for the finite endpoint inversion and for the fallback from a stale trim.
Until that proof exists, the bounded endpoint result remains a CADIR admission
witness rather than a proven global minimum.

**Note.** QA audit: commit `ed6dd2432` closed this item by declaring the
bounded heuristic a CADIR admission rule. This pass settles the format's
candidate-set and connectivity meaning and replaces numerical tie selection
with conservative omission, but it deliberately leaves the endpoint-proof
remainder open.

## 6. Units and measures

## 7. Annotation, presentation, and tessellation
