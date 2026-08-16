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

## 2. AP242 BO-Model sidecars

## 3. Containers and other encodings

### CE-03. Part 28 XML grammar

**Question.** What XML grammar represents an AP203, AP214, or AP242 exchange structure in Part 28?

**Known.** ISO 10303-28 defines an EXPRESS-to-XML mapping and supplies the
generic XML mapping rules. AP203, AP214, and AP242 still require the matching
edition's EXPRESS schema and generated XML schema components. Part 21 syntax
does not identify those XML namespaces or schema resources.

**Note.** A generic XML parser cannot establish the AP schema, select the
edition, or decode schema-specific XML constructs without those inputs.

**Need.** We must know the grammar to parse record boundaries, values, and references from Part 28 XML.

### CE-04. Part 28 graph mapping

**Question.** How does each Part 28 XML construct map to the entity graph and invariants in `step.md`?

**Known.** ISO 10303-28 defines the mapping from EXPRESS entities and values
to XML elements, attributes, namespaces, and references. The mapping is
schema-driven; it is not a second universal Part 21 record grammar.

**Note.** Applying the mapping needs the exact AP schema, XML schema version,
and a mapping implementation for every supported construct. No such Part 28
input or IR adapter exists in this codec.

**Need.** We must know the mapping to apply schema decoding to a Part 28 exchange structure.

### CE-05. Part 26 binary grammar

**Question.** What HDF5 layout represents an AP203, AP214, or AP242 exchange structure in Part 26?

**Known.** ISO/TS 10303-26 defines an EXPRESS-driven HDF5 mapping. The HDF5
layout is schema- and mapping-version dependent; the HDF5 signature alone
does not identify AP203, AP214, or AP242.

**Note.** The codec has no Part 26 mapping tables or HDF5 reader.

**Need.** We must know the layout to parse record boundaries, values, and references from Part 26 data.

### CE-06. Part 26 graph mapping

**Question.** How does each Part 26 HDF5 construct map to the entity graph and invariants in `step.md`?

**Known.** Part 26 defines the schema-driven mapping from EXPRESS values and
entity identity to HDF5 datasets, datatypes, and links. It does not make every
HDF5 dataset self-describing as an AP203, AP214, or AP242 instance graph.

**Note.** Applying the mapping needs the exact Part 26 mapping version and AP
schema. No such Part 26 input or IR adapter exists in this codec.

**Need.** We must know the mapping to apply schema decoding to a Part 26 exchange structure.

## 4. Signatures

### SG-04. Signature verification result

**Question.** Which validation conditions make each signature valid, invalid, or indeterminate?

**Known.** Part 21 requires a detached CMS `SignedData` object and defines
the exact external content and alphabet projection. RFC 5652 supplies the
message-digest, signed-attribute, signature, certificate, and algorithm
processing rules. A valid result also needs a certificate-chain, key-usage,
revocation, time, and trust-anchor policy. An invalid result covers malformed
CMS, a digest mismatch, a signature mismatch, or a failed required policy;
indeterminate covers unavailable content, keys, certificates, or policy
evidence.

**Note.** Part 21 does not prescribe a trust store, revocation protocol,
clock policy, or caller authorization policy. Verification therefore needs a
caller-supplied CMS and trust-policy contract; structural CMS parsing alone
cannot produce a valid/invalid result.

**Need.** We must know the conditions to report a signature verification result.

## 5. Topology and pcurve decisions

## 6. Units and measures

## 7. Annotation, presentation, and tessellation
