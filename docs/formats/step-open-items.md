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

### CE-03. Part 28 XML grammar

**Question.** What XML grammar represents an AP203, AP214, or AP242 exchange structure in Part 28?

**Known.** ISO 10303-28 defines an EXPRESS-to-XML mapping and supplies the
generic XML mapping rules. AP203, AP214, and AP242 still require the matching
edition's EXPRESS schema and generated XML schema components. Part 21 syntax
does not identify those XML namespaces or schema resources.

**Note.** A generic XML parser does not establish the AP schema, select the
edition, or decode schema-specific XML constructs. Read the matching EXPRESS
schema and its generated XML schema components from the published stepmod or
stepcode repository and the AP242 downloads.

**Need.** We must know the grammar to parse record boundaries, values, and references from Part 28 XML.

### CE-04. Part 28 graph mapping

**Question.** How does each Part 28 XML construct map to the entity graph and invariants in `step.md`?

**Known.** ISO 10303-28 defines the mapping from EXPRESS entities and values
to XML elements, attributes, namespaces, and references. The mapping is
schema-driven; it is not a second universal Part 21 record grammar.

**Note.** Applying the mapping needs the exact AP schema and XML schema
version, both published, and a mapping implementation for every supported
construct. Read ISO 10303-28, then author the Part 28 witness input and the
IR adapter.

**Need.** We must know the mapping to apply schema decoding to a Part 28 exchange structure.

### CE-05. Part 26 binary grammar

**Question.** What HDF5 layout represents an AP203, AP214, or AP242 exchange structure in Part 26?

**Known.** ISO/TS 10303-26 defines an EXPRESS-driven HDF5 mapping. The HDF5
layout is schema- and mapping-version dependent; the HDF5 signature alone
does not identify AP203, AP214, or AP242.

**Note.** Derive the Part 26 mapping tables and the HDF5 reader from
ISO/TS 10303-26.

**Need.** We must know the layout to parse record boundaries, values, and references from Part 26 data.

### CE-06. Part 26 graph mapping

**Question.** How does each Part 26 HDF5 construct map to the entity graph and invariants in `step.md`?

**Known.** Part 26 defines the schema-driven mapping from EXPRESS values and
entity identity to HDF5 datasets, datatypes, and links. It does not make every
HDF5 dataset self-describing as an AP203, AP214, or AP242 instance graph.

**Note.** Applying the mapping needs the exact Part 26 mapping version and AP
schema from the published texts. Read ISO/TS 10303-26, then author the Part 26
witness input and the IR adapter.

**Need.** We must know the mapping to apply schema decoding to a Part 26 exchange structure.

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

### DR-01. Drawing target identity selection

**Question.** Which neutral identity represents a drawing reference when one
STEP source record maps to more than one neutral model identity?

**Known.** `record_targets` collects every neutral identity derived from a
source record into a `BTreeSet`. `target_for` transfers a target only when the
set has exactly one identity; an ambiguous typed target returns no target and
the caller records `drawing.relationship-target-ambiguous`
(`crates/cadmpeg-codec-step/src/reader/drawing.rs:459-506,786-840`). The
refusal preserves the raw source parameter but does not identify the source
rule for a multi-view drawing reference.

**Note.** The later implementation removes lexicographic selection, but a
conservative ambiguity loss is not a source identity rule. The presentation
layer expands all applicable product views, but the drawing target path does
not. No existing STEP item settles this projection.

**Need.** We need the drawing-reference target entity and product-definition
scope rule, plus a multi-view witness file that shows whether all views
are targets, one view is authoritative, or the reference is ambiguous.

QA audit: reopened after reviewing closing commit 4878c68ea. The closing commit preserves an ambiguous target and records a loss. This is safe refusal behavior, not evidence of which source identity a multi-view drawing reference owns.
