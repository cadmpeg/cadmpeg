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

### TP-09. Global pcurve admission

**Question.** What invariant permits a non-seam pcurve to become a typed
coedge relation when the source provides no selector between same-surface
candidates?

**Known.** ISO 10303-42 requires the selected pcurve to describe the same
model-space point set and direction as its `curve_3d`. The decoder accepts one
same-surface candidate after finite endpoint fitting and a finite locus
witness. `crates/cadmpeg-codec-step/src/reader/topology.rs:3098-3303` uses 64
endpoint subdivisions, bounded Newton searches, and 23 locus samples plus
breakpoints. `step.md` §5 "CADIR decision: a typed `SEAM_EDGE` uses its explicit
pcurve only" defines this as an admission witness and states that it does not
prove global equality.

**Need.** Provide an exact or validated adaptive point-set bound for typed
admission, or make the finite witness an explicit lossy boundary that cannot
claim pcurve fidelity. Add a witness with a narrow interior divergence
between every sampled parameter and a witness with a missed lower-residual
endpoint basin.

**Conflict.** The source invariant is global point-set equality, but the
decoder can transfer a typed pcurve after finite samples that do not constrain
the unsampled interval.

**Note.** QA audit of commit `b28d82721c4ee1f48a389a5a100f9fb167885276`.
Closures `716f93d8bf372ef41be015f95fecb30a47e9853e` and
`4ff650aa39ddb4abd5f087dbfd9bac579adc925c` promoted the finite witness.
The current tests cover crossing and missed-sample behavior, but do not show
that a passing finite witness preserves the source point set.

## 6. Units and measures

## 7. Annotation, presentation, and tessellation

## 8. Product structure and placement

### PS-04. Product-definition view order

**Question.** What order, if any, may a neutral presentation layer use for
multiple `PRODUCT_DEFINITION` views of one `PRODUCT`?

**Known.** `PRODUCT_DEFINITION_FORMATION.of_product` and
`PRESENTATION_LAYER_ASSIGNMENT.assigned_items` are `SET`s, so neither source
relationship supplies view order. `crates/cadmpeg-codec-step/src/reader/product.rs:73-89`
groups definitions and sorts them by `record.span.start`.
`crates/cadmpeg-codec-step/src/reader/presentation.rs:214-227` preserves that
expanded order, and `crates/cadmpeg-codec-step/src/reader/presentation.rs:787-792`
emits it as layer item order. `step.md` §8 "ISO 10303-41 defines a
`PRODUCT_DEFINITION` as one aspect or view" calls this DATA-order projection
but also states that STEP supplies no view order.

**Need.** Provide a source-defined view-order rule or make the neutral order
non-semantic. Add a producer or validator witness that permutes DATA records
while preserving all relationships and proves that downstream consumers do
not observe a changed presentation order, or remove order from the neutral
contract.

**Conflict.** The specification says that the source relationships do not
order views, but the decoder makes DATA serialization order observable in
`PresentationLayer.items`.

**Note.** QA audit of commit `b28d82721c4ee1f48a389a5a100f9fb167885276`.
Closures `29067a7725343e0f849135bf90aee04ed0ec9cd4` and
`225b25fb6b9de5b9795b7d4667eca64884aa2d33` changed and tested byte-span
ordering. Their reordered fixtures keep identities and relationships but
change layer item order, so they prove the implementation policy rather than
a STEP source rule.
