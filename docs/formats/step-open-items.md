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

### CE-02. ZIP resource composition

**Question.** How do references between exchange resources in an edition-3 ZIP container resolve into one graph?

**Known.** Annex A.4 defines archive-relative addressing and root-anchor forwarding. The archive reader checks the addressed member but admits only `ISO-10303.p21` to semantic decode. It does not substitute an entity or value from a subsidiary graph.

**Need.** We need a multi-member archive witness and a resource-qualified graph model that verifies forwarded anchor substitution.

**Note.** QA audit: commit `2a5fd8ce8` documented root-only decode and deleted the item. Root-only refusal is not the resource-composition operand.

### CE-03. Part 28 XML grammar

**Question.** Which Part 28 configuration and generated schema define each supported AP203, AP214, or AP242 XML grammar?

**Known.** Part 28 is an EXPRESS-to-XML mapping. The current codec detects a Part 28 candidate and refuses it before Part 21 parsing. It has no Part 28 schema or decoder.

**Need.** We need the supported AP schema, Part 28 configuration, generated XML schema, and independent conforming exchanges.

**Note.** QA audit: commit `8fd1526d3` closed this item by documenting unsupported-input refusal. Detection and refusal do not decode the grammar.

### CE-04. Part 28 graph mapping

**Question.** How does each supported Part 28 XML construct map to the STEP instance graph and CADIR?

**Known.** The mapping is schema-driven. The codec has no XML entity, value, reference, or identity adapter.

**Need.** We need the exact mapping configuration and conformance witnesses for entity identity, references, aggregates, and omitted values.

**Note.** QA audit: commit `8fd1526d3` closed this item with the same unsupported-input refusal as CE-03. No graph-mapping operand executes.

### CE-05. Part 26 binary grammar

**Question.** Which Part 26 mapping and HDF5 layout define each supported AP203, AP214, or AP242 binary exchange?

**Known.** Part 26 is an EXPRESS-driven HDF5 mapping. The codec detects the HDF5 signature and refuses the input. It has no Part 26 mapping tables or HDF5 reader.

**Need.** We need the supported AP schema, mapping version, HDF5 layout, and independent conforming exchanges.

**Note.** QA audit: commit `8fd1526d3` closed this item by documenting unsupported-input refusal. Signature detection does not decode the grammar.

### CE-06. Part 26 graph mapping

**Question.** How does each supported Part 26 construct map to the STEP instance graph and CADIR?

**Known.** The mapping uses schema and population groups, compound entity types, optional bitmaps, instance identifiers, aggregates, and links. The codec implements none of these mappings.

**Need.** We need the exact mapping version and conformance witnesses for identity, references, aggregates, and optional values.

**Note.** QA audit: commit `8fd1526d3` closed this item with the same unsupported-input refusal as CE-05. No graph-mapping operand executes.

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
