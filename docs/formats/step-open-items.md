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

### ER-02. Resource access

**Question.** Which retrieval and authentication procedure applies to each external resource URI?

**Known.** Part 21 defines the required result of a resource resolution, but
it does not define a transport, authentication mechanism, authorization scope,
redirect policy, certificate policy, or registry protocol. A fragment-only
UUID may require a registry service.

**Note.** Resource retrieval and authentication are outside the codec contract.

**Need.** We must know the procedure to obtain the selected external resource.

### ER-03. Resource composition

**Question.** How does each external resource combine with the local instance graph?

**Known.** Part 21 resolves a resource fragment to an entity or value ANCHOR,
follows forwarded URI anchors, and returns `$` on a failed or cyclic
resolution. It does not define a merge of two independent numeric DATA
namespaces. The neutral IR currently has one document-wide identity universe.

**Note.** A resource-qualified graph import needs an explicit identity,
schema, unit, and trust policy. The reader retains unresolved external
occurrences and does not invent a cross-resource merge.

**Need.** We must know the composition rule to resolve cross-resource identities and build one product graph.

### ER-04. Resource cache identity

**Question.** Which URI components and resource metadata determine whether two external resource references identify the same cached resource?

**Known.** Each REFERENCE entry contains a URI. Part 21 does not define cache
keys, freshness, validators, content negotiation, or equivalence of two
representations returned for one URI.

**Note.** Cache identity is a transport and resource-policy decision. The
codec does not cache or combine retrieved resources.

**Need.** We must know the identity rule to reuse a retrieved resource without combining different resources.

## 2. AP242 BO-Model sidecars

### BM-01. Sidecar envelope

**Question.** What XML grammar and file relationship identify an AP242 BO-Model sidecar?

**Known.** AP242 BO-Model XML is a separate AP242 encoding with its own XML
schema and edition-specific document envelope. Part 21 has no required
sidecar filename, XML root, content identifier, or association record that
binds such a document to one Part 21 exchange.

**Note.** Detecting an XML root string cannot establish the AP242 edition or
the relationship to a Part 21 file. A schema and application-level
association contract are required.

**Need.** We must know the envelope to detect, parse, and associate the sidecar with its Part 21 exchange structure.

### BM-02. Sidecar composition

**Question.** How do AP242 BO-Model XML identities and values combine with the Part 21 instance graph?

**Known.** The BO-Model XML schema and Part 21 encode related product data in
different representation systems. Part 21 identity numbers are local to its
DATA graph. No Part 21 rule maps an XML object identity or XML value to a
numeric Part 21 instance.

**Note.** Sidecar composition needs a declared AP242 XML edition, identity
linkage, precedence rule, and conflict policy. The STEP codec has no such
contract.

**Need.** We must know the composition rule to build one product graph from the Part 21 exchange structure and its sidecar.

## 3. Containers and other encodings

### CE-02. ZIP resource composition

**Question.** How do references between exchange resources in an edition-3 ZIP container resolve?

**Known.** Annex A.4 defines the container rule: a relative URI is resolved
against the directory of the referencing member, `..` cannot escape the
archive, and only `ISO-10303.p21` can be referenced from outside. Dot segments
are removed. The root reader applies this rule to its REFERENCE entries and
checks each resolved internal member. A root ANCHOR forwards a reference to an
entity or value in a subsidiary member. The rule does not define how a
subsidiary exchange graph is merged into the root graph.

**Note.** The reader implements path resolution and root-member checks. It
does not import subsidiary DATA graphs because the neutral IR has no
resource-qualified identity universe or cross-file schema/unit merge policy.

**Need.** We must know the resolution rule to combine the contained resources into one product graph.

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
