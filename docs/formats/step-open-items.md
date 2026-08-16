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

### ER-01. URI resolution

**Question.** Which base URI and normalization rules apply to each relative URI in a REFERENCE section or a document-reference entity?

**Known.** A Part 21 resource is an IETF URI. A fragment-only URI whose
fragment is not a UUID selects the same-named local ANCHOR. A URI with a
resource path is relative to the resource that contains the reference; in a
ZIP archive, Annex A.4 makes that the directory of the referencing member and
forbids traversal above the archive root. A standalone file's transport base
URI is not encoded in Part 21. Application document-reference attributes do
not define a Part 21 base URI.

**Note.** Local fragment resolution is implemented. The base URI for a
standalone input, URI normalization supplied by its transport, and the
interpretation of application document-reference paths remain caller policy.

**Need.** We must know the rules to identify the external resource that a relative URI selects.

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

### TP-03. Non-planar pcurve units

**Question.** Which scale does each pcurve axis use for elementary and swept
support surfaces, including the directrix and extrusion-vector cases?

**Known.** ISO 10303-42 defines a pcurve in its support surface's `(u,v)`
parameter space. The analytic surface equations identify plane axes from the
representation length context, cylinder and cone axes as `(plane angle,
length)`, and sphere and torus axes as `(plane angle, plane angle)`. A linear
extrusion is `lambda(u) + v V`; the referenced curve defines `u` and the
extrusion vector magnitude defines the parameterization for `v`. A revolution
uses plane angle for `u` and the referenced curve's parameter for `v`.

**Note.** Commit `f41f2898c` promoted a scale table into `docs/formats/step.md`
and removed this item. The implementation and `reader/geometry.rs` tests use
synthetic IR variants. They do not establish the directrix unit mapping for
every curve kind. In particular, the decoder scales the extrusion vector to
document units and also applies a document length scale to the pcurve's
second axis. The source equation does not by itself justify treating that
axis as an independent length coordinate. This item is reopened.

**Need.** We need a parameter-scale table that preserves the source
parameterization and vector magnitudes for every supported directrix and
surface wrapper, derived from the ISO 10303-42 equations and checked against
exporter-authored witness files, before this rule is settled.

### TP-09. Pcurve endpoint and tied-locus verification

**Question.** What evidence proves that a non-seam pcurve candidate is the
correct edge carrier, and that tied candidates have the same model-space
locus?

**Known.** `select_associated_pcurve` scores candidate endpoint fits and
accepts the lowest finite score within tolerance
(`crates/cadmpeg-codec-step/src/reader/topology.rs:3623-3747`). A tie is
declared from a relative score threshold, and `pcurve_loci_equivalent`
compares 33 samples in each direction before the first tied candidate is
selected. The search uses a finite seed set and a bounded iterative closest
point calculation.

**Note.** TP-02 records the semantic selection rule, but this implementation
does not prove a global minimum or a global locus equivalence. A pcurve with
an unsampled endpoint minimum, or two distinct curves that meet at the sample
points and diverge between them, can pass the acceptance checks. The first
tied candidate then depends on candidate order. This item records the
verification gap rather than treating the numerical heuristic as STEP
semantics.

**Need.** We need multi-pcurve witness files, authored with an available
exporter or taken from a public corpus, and an exact inverse or
interval/adaptive proof for endpoint fit and locus equivalence, including
reordered, near-tied, and crossing candidates.

## 6. Units and measures

## 7. Annotation, presentation, and tessellation

### DR-01. Drawing target identity selection

**Question.** Which neutral identity represents a drawing reference when one
STEP source record maps to more than one neutral model identity?

**Known.** `record_targets` collects every neutral identity derived from a
source record into a `BTreeSet`
(`crates/cadmpeg-codec-step/src/reader/mod.rs:1038-1051`). The drawing target
resolver returns `identities.iter().next()` and does not retain the remaining
identities (`crates/cadmpeg-codec-step/src/reader/drawing.rs:406-444`). A
source record with multiple product-definition views can therefore receive
the lexicographically first identity.

**Note.** This is an ownership inference from neutral identity ordering. If a
drawing annotation or presentation reference targets a source record with two
valid product-definition identities, changing identity spelling or insertion
order can retarget the drawing without changing the STEP reference. The
presentation layer expands all applicable product views, but the drawing
target path does not. No existing STEP item settles this projection.

**Need.** We need the drawing-reference target entity and product-definition
scope rule, plus an independent multi-view file that shows whether all views
are targets, one view is authoritative, or the reference is ambiguous.
