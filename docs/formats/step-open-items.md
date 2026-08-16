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
interpretation of application document-reference paths need an explicit
policy recorded as a CADIR decision.

**Need.** We must know the rules to identify the external resource that a relative URI selects.

### ER-02. Resource access

**Question.** Which retrieval and authentication procedure applies to each external resource URI?

**Known.** Part 21 defines the required result of a resource resolution, but
it does not define a transport, authentication mechanism, authorization scope,
redirect policy, certificate policy, or registry protocol. A fragment-only
UUID may require a registry service.

**Note.** Resource retrieval and authentication need an explicit policy
recorded as a CADIR decision.

**Need.** We must know the procedure to obtain the selected external resource.

### ER-03. Resource composition

**Question.** How does each external resource combine with the local instance graph?

**Known.** Part 21 resolves a resource fragment to an entity or value ANCHOR,
follows forwarded URI anchors, and returns `$` on a failed or cyclic
resolution. It does not define a merge of two independent numeric DATA
namespaces. The neutral IR currently has one document-wide identity universe.

**Note.** A resource-qualified graph import needs an explicit identity,
schema, unit, and trust policy recorded as a CADIR decision. The reader
retains unresolved external occurrences and does not invent a cross-resource
merge.

**Need.** We must know the composition rule to resolve cross-resource identities and build one product graph.

### ER-04. Resource cache identity

**Question.** Which URI components and resource metadata determine whether two external resource references identify the same cached resource?

**Known.** Each REFERENCE entry contains a URI. Part 21 does not define cache
keys, freshness, validators, content negotiation, or equivalence of two
representations returned for one URI.

**Note.** Cache identity is a transport and resource policy recorded as a
CADIR decision. The codec does not cache or combine retrieved resources.

**Need.** We must know the identity rule to reuse a retrieved resource without combining different resources.

## 2. AP242 BO-Model sidecars

### BM-01. Sidecar envelope

**Question.** What XML grammar and file relationship identify an AP242 BO-Model sidecar?

**Known.** AP242 BO-Model XML is a separate AP242 encoding with its own XML
schema and edition-specific document envelope. Part 21 has no required
sidecar filename, XML root, content identifier, or association record that
binds such a document to one Part 21 exchange.

**Note.** Detecting an XML root string does not establish the AP242 edition
or the relationship to a Part 21 file. The BO-Model XML schema comes from the
published AP242 downloads; the association rule comes from the CAx-IF
recommended practices.

**Need.** We must know the envelope to detect, parse, and associate the sidecar with its Part 21 exchange structure.

### BM-02. Sidecar composition

**Question.** How do AP242 BO-Model XML identities and values combine with the Part 21 instance graph?

**Known.** The BO-Model XML schema and Part 21 encode related product data in
different representation systems. Part 21 identity numbers are local to its
DATA graph. No Part 21 rule maps an XML object identity or XML value to a
numeric Part 21 instance.

**Note.** Sidecar composition needs the declared AP242 XML edition from the
published AP242 downloads, plus an identity linkage, precedence rule, and
conflict policy recorded as a CADIR decision.

**Need.** We must know the composition rule to build one product graph from the Part 21 exchange structure and its sidecar.

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
clock policy, or authorization policy. Verification therefore needs an
explicit trust policy recorded as a CADIR decision; structural CMS parsing
alone does not produce a valid/invalid result.

**Need.** We must know the conditions to report a signature verification result.

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

### SG-03. Signature value encoding

**Known.** The SIGNATURE section contains RFC 4648 Base64 text. Decoding
produces the CMS `SignedData` object described by RFC 5652, including its
signer information and any certificates or algorithm parameters carried by
that object. The parser validates the Base64 framing and retains the raw
section, payload range, and decoded CMS bytes.

**Question.** How does each signature method encode its signature value and verification material in the SIGNATURE section?

`step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." through `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." require retention of the complete SIGNATURE byte range. The specification gives no field grammar for the retained content.

**Need.** We must know the encoding to extract the signature value, keys, certificates, and method parameters.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### TP-01. Shared-edge ownership

**Question.** What STEP rule establishes shared-edge ownership?

**Known.** A distinct committed topology root is an ownership boundary. If
one distinct root key exists, source edge and vertex identities remain shared
within that root. If multiple distinct root keys exist, every root scopes its
shell, edge, and vertex identities by the root instance. Aliases with the same
root key reuse the committed body. A root with multiple shell owners also
scopes carriers by shell. The reader does not invent sharing between
independent roots and does not make identity selection depend on source record
order.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### TP-02. Seam pcurve selection

**Question.** What STEP rule establishes seam pcurve selection?

**Known.** A `SEAM_EDGE` supplies the authoritative pcurve reference. The
reference must be a decoded `PCURVE` in the edge's `SEAM_CURVE` associated
geometry and on the coedge's face surface. The reader does not replace an
invalid seam reference with an endpoint or serialization-order guess. A
non-seam oriented edge uses endpoint continuity only when one same-surface
pcurve is selected, or when tied candidates have the same model-space locus;
distinct unresolved candidates remain detached and produce a topology loss.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### TP-05. Partial solid and tolerant point carriers

**Question.** What STEP rule establishes partial solid and tolerant point carriers?

**Known.** CADIR has no tolerant-point or partial-solid carrier. A
`VERTEX_POINT` without a resolvable `CARTESIAN_POINT`, and every solid root
with a missing mandatory carrier, is rejected atomically. The reader retains
the source records as opaque data and emits a `TopologyNotTransferred` error;
it does not infer coordinates or create a partial body. Salvage applies only
to independent sheet or wire members that are complete.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### TP-06. Implicit face-plane orientation

**Question.** What STEP rule establishes implicit face-plane orientation?

**Known.** A base `FACE` without a surface uses the first outer boundary,
or the first valid boundary when no outer role exists. Its signed ring area
defines the normal. The centroid defines the origin, and the projection of the
most orthogonal global coordinate axis defines the u-axis, with x/y/z tie
order. The ring must be planar within the document coincidence tolerance and
`1e-12` of its scale. Degenerate or non-planar rings reject the topology root;
an `ORIENTED_FACE` still composes reversal through the face sense and boundary
traversal. This makes the inferred carrier independent of cyclic ring
serialization and prevents a non-planar boundary from receiving a fabricated
plane.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### TP-07. Pcurve recursion and normalization

**Question.** What STEP rule establishes pcurve recursion and normalization?

**Known.** A `PCURVE` definition must resolve to exactly one item in its
`DEFINITIONAL_REPRESENTATION`. The reader decodes 2D line, analytic conic,
polyline, NURBS, trimmed, offset, and affine-replica carriers. A 2D line
uses its referenced point and vector directly; its coordinates are then
converted once into the owning surface chart. A recursive carrier returns no
typed geometry when an active record repeats or the graph reaches depth 256;
the active set is released on every return path. Unsupported composite or
otherwise unrecognized 2D carriers remain named opaque records and are not
attached to a coedge. Topology that needs such a carrier records a
machine-readable pcurve omission loss.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### TP-08. Face-bound partial dispatch

**Question.** What STEP rule establishes face-bound partial dispatch?

**Known.** The partial with the boundary parameters supplies the inherited
`FACE_BOUND` attributes. An empty `FACE_OUTER_BOUND` partial supplies the
outer-role classification.

`has_type` matches a partial name exactly and does not walk the
EXPRESS subtype hierarchy
(`crates/cadmpeg-codec-step/src/reader/topology.rs:4633`). Two sites choose
the governing partial in opposite orders. The shell reader tries `FACE_BOUND`
first (`topology.rs:2205`). The implicit-plane reader tries `FACE_OUTER_BOUND`
first (`topology.rs:2978`). `FACE_OUTER_BOUND` adds no attributes to
`FACE_BOUND`, so the second site reads attribute 1 of an empty partial and
returns no loop.

Face-bound classification reads the presence of
`FACE_OUTER_BOUND`, while attribute lookup selects the first face-bound
partial that carries the three boundary parameters. The shell reader and
implicit-plane reader use this same dispatch. The synthesized complex-face
fixture covers the inherited-attribute form.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PC-04. Chart write-back to the shared pcurve

**Question.** What STEP rule establishes chart write-back to the shared pcurve?

**Known.** The source pcurve carrier is immutable. A chart variant derived
from one coedge's endpoint fit is a use-scoped pcurve carrier. The coedge owns
the derived carrier through its `PcurveUse`; another coedge may select a
different variant without changing the source carrier or the first coedge's
parameter range.

If selection keeps the source geometry, the coedge references the
source pcurve. If selection changes the geometry, the reader creates a
canonical use-scoped pcurve identity and copies the source carrier metadata.
The source pcurve remains available for other uses and for opaque-record
ownership. When no typed use retains the source identity, normal carrier
retention removes the unowned neutral source carrier while preserving its raw
STEP record as opaque data.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### EL-01. Character encoding selection

**Question.** What STEP rule establishes character encoding selection?

**Known.** The major value in the raw `FILE_DESCRIPTION`
`implementation_level` selects the direct string repertoire. Values `4;1`,
`4;2`, and `4;3` use UTF-8. Earlier implementation levels use ISO-8859-1.
The reader applies this selection to every semantic string and retains
`\X2\` and `\X4\` escape decoding in both repertoires. Invalid direct UTF-8
bytes produce a metadata loss.

`step.md` §2 and §6 define the repertoire and its header selector.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

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

### TP-03. Non-planar pcurve units

**Question.** Which scale does each pcurve axis use for elementary and swept
support surfaces, including the directrix and extrusion-vector cases?

**Known.** ISO 10303-42 defines a pcurve in its support surface's `(u,v)`
parameter space. The current reader applies representation scales and the
surface table in `crates/cadmpeg-codec-step/src/reader/geometry.rs:4432-4614`:
planes use length/length, cylinders and cones use angle/length, spheres and
toruses use angle/angle, NURBS uses dimensionless axes, and swept surfaces
derive axes from the directrix. The source curve scale table is at
`geometry.rs:207-303`.

**Note.** The later implementation adds a scale table and preserves unknown
procedural cases, but the directrix parameter mapping for every supported curve
kind is still an implementation assumption. The source equation alone does
not establish each curve's native parameter units. This item remains open.

**Need.** We need a parameter-scale table that preserves the source
parameterization and vector magnitudes for every supported directrix and
surface wrapper, derived from the ISO 10303-42 equations and checked against
exporter-authored witness files, before this rule is settled.

QA audit: reopened after reviewing closing commit cbbbe401b. The closing commit adds a parameter-scale table and synthetic scale tests, but directrix parameterization and unsupported curve families still rely on implementation assumptions.
