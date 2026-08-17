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

### AP-12. Presentation PMI placement arbitration

**Question.** Which source attributes can supply the placement of a
presentation annotation?

**Known.** ISO 10303-46 names `ANNOTATION_TEXT.mapping_target`,
`DEFINED_CHARACTER_GLYPH.placement`, and `TEXT_LITERAL.placement` as
placement attributes. `crates/cadmpeg-codec-step/src/reader/pmi.rs:565-607`
collects placement candidates from the annotation graph, and
`crates/cadmpeg-codec-step/src/reader/pmi.rs:1239-1276` treats every reachable
record present in `geometry.placements` as a candidate. One candidate is
transferred; multiple candidates cause omission and an ambiguity loss.

**Need.** Traverse the typed placement-carrier attributes, or state and prove
a source rule that every reachable placement-bearing record is a carrier. Add
a graph with one valid annotation placement and one unrelated referenced
auxiliary record that contains an `AXIS2_PLACEMENT_3D`; the valid placement
must remain transferable and the auxiliary record must remain opaque.

**Conflict.** The specification names typed placement fields, but the decoder
uses reachability alone. An unrelated placement can therefore create a false
ambiguity and omit a valid placement, or become the only transferred
placement.

**Note.** QA audit of commit `b28d82721c4ee1f48a389a5a100f9fb167885276`.
Closure `490e0d38f55033a07534a42d3a854e0967574841` removed first-candidate
selection and tested multiple direct text-placement references. It did not
test an unrelated placement-bearing record in the reachable graph.

### TS-02. Tessellation product-link admission

**Question.** Which representation relation admits a tessellated shape
representation as a product-linked representation?

**Known.** A product-linked representation is seeded by
`SHAPE_DEFINITION_REPRESENTATION`. `crates/cadmpeg-codec-step/src/reader/tessellation.rs:584-637`
then expands that set through both `REPRESENTATION_RELATIONSHIP` and
`SHAPE_REPRESENTATION_RELATIONSHIP`. The tessellation admission at
`crates/cadmpeg-codec-step/src/reader/tessellation.rs:90-119` transfers a
bodyless mesh when this expanded set contains its representation.
`crates/cadmpeg-codec-step/src/reader/topology.rs:143-176` uses only the
typed shape relationship for body ownership. The specification states that a
generic `REPRESENTATION_RELATIONSHIP` relates representations but does not
make one part of the other (`step.md` §8 "ISO 10303-43 defines uncertainty at
three scopes.").

**Need.** Define the direct relation that admits a product-linked
tessellation. Add a negative fixture with a product-linked exact
representation, an unrelated tessellated representation, and only a generic
`REPRESENTATION_RELATIONSHIP` between them. The unrelated mesh must remain a
detached source association. Add the positive typed
`SHAPE_REPRESENTATION_RELATIONSHIP` case separately.

**Conflict.** Generic representation reachability currently admits a
tessellated representation, although the specification excludes that generic
relation from part ownership and the body-owner index does not use it as a
typed shape relation.

**Note.** This is a new item from the hostile sweep of commit
`b28d82721c4ee1f48a389a5a100f9fb167885276`. Existing tessellation tests cover
direct typed shape relationships at
`crates/cadmpeg-codec-step/src/reader/tessellation/tests.rs:452-602`; they do
not cover a generic-relation bridge.

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
