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

### UD-01. User-defined entity semantics

**Known.** Part 21 does not assign semantics to a user-defined entity
name. The number, data types, and meanings of its attributes are an agreement
between the exchange partners. The reader therefore retains the complete
record as a named opaque record with its links and does not infer a native or
neutral entity.

**Question.** What entity semantics does each user-defined `!` entity name select?

`step.md` §3 "user_name" defines the syntax of a user-defined name. `step.md` §5 "Entity instance names share one namespace across all DATA sections." through `step.md` §5 "Entity instance names share one namespace across all DATA sections." require an unknown entity to retain its name, complete spans, and links to other named opaque records.

**Need.** We must know the semantics to transfer a user-defined entity to typed native or neutral records.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### UD-02. User-defined type semantics

**Known.** Part 21 does not assign semantics to a user-defined type name.
The wrapped parameter remains a typed opaque value; the partners' agreed
schema selects its type. The reader preserves the type name, wrapped value, record span,
and links and does not select a neutral value type by name alone.

**Question.** What value semantics does each user-defined `!` type name select?

`step.md` §3 "user_name" defines the syntax of a user-defined name. `step.md` §5 "A parameter is an entity reference, value reference, named entity constant," through `step.md` §5 "A parameter is an entity reference, value reference, named entity constant," define a typed parameter as a name with one parameter.

**Need.** We must know the semantics to decode the wrapped parameter as a typed value.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### SG-01. Signature method selection

**Known.** Part 21 defines one signature method: a detached CMS
`SignedData` object. The digest and signature algorithm identifiers are inside
the decoded CMS object, not in a Part 21 field. The parser records the decoded
CMS payload and does not guess an algorithm from its bytes.

**Question.** Which SIGNATURE field identifies the signature method and its parameters?

`step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." through `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." define the complete byte boundary of a SIGNATURE section. The specification gives no field grammar for its content.

**Need.** We must know the selection rule to choose the correct signature verification method.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### SG-02. Signed byte sequence

**Known.** A signature covers the Part 21 alphabet bytes from the first
`ISO-10303-21;` token through the byte before its `SIGNATURE;` token. The
alphabet filter removes transport controls and retains the permitted Part 21
characters. Each later signature covers the earlier signature sections too.
The parser records this source range for every signature.

**Question.** Which exact bytes does each signature method authenticate?

`step.md` §2 "A clear-text exchange structure uses this outer grammar:" through `step.md` §2 "A clear-text exchange structure uses this outer grammar:" place each SIGNATURE section after the exchange terminator. `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." through `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." define the byte boundary of the SIGNATURE section.

**Need.** We must know the byte sequence to calculate the verification input.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

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

### UM-01. Unit context selection

**Question.** What STEP rule establishes unit context selection?

**Known.** Each representation's `GLOBAL_UNIT_ASSIGNED_CONTEXT` supplies
the length and plane-angle scales for that representation and its reachable
representation-item closure. A carrier shared by representations must have
one equal scale in every context. A conflicting carrier has no per-carrier
override, uses the document fallback scale, and produces a geometry loss.
Unscoped values use the document fallback scale. The resolved scales reach
geometry, PMI, tessellation, topology, and validation consumers.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### UM-02. Representation uncertainty selection

**Question.** What STEP rule establishes representation uncertainty selection?

**Known.** The linear tolerance is the `UNCERTAINTY_MEASURE_WITH_UNIT`
whose unit resolves to a length unit. If several length measures are present,
the measure named `distance_accuracy_value` takes precedence. Without that
name, exactly one length measure is required. An angular measure does not
block a later length measure, and an ambiguous set produces a machine-readable
geometry loss instead of selecting by source order.

`step.md` §8 defines the length-unit invariant and selection rule.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### UM-03. SI prefix on plane-angle units

**Question.** What STEP rule establishes si prefix on plane-angle units?

**Known.** An SI prefix applies to a plane-angle `SI_UNIT` before the
unit is converted to radians. An omitted prefix has factor 1.

`step.md` §8 "SI prefixes apply before conversion-based-unit
factors." states the rule without restriction to a unit kind.

The angular unit resolver reads the optional prefix in parameter 0,
uses the same SI prefix factors as the length resolver, and multiplies the
resulting factor into conversion-based-unit factors. The rule is covered by
the parser-level `MILLI` and omitted-prefix regression cases in
`reader/geometry.rs`.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PC-01. Angular parameter unit repair

**Question.** What STEP rule establishes angular parameter unit repair?

**Known.** A pcurve does not have its own angular unit. Its coordinates use
the parameterization of its owning surface after the representation and
record-specific unit scales have been applied. The reader does not generate
degree/radian alternatives or rescale an angular axis from endpoint evidence.
If the declared coordinates do not form a usable topological carrier, the
coedge remains without a pcurve and the machine-readable topology loss records
that omission.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PC-02. Synthesized pcurve chart

**Question.** May the decoder replace a pcurve's parameterization with an
affine map that it derives from the edge endpoints?

**Known.** Endpoint-derived calibration is allowed only when it preserves
every source coordinate. A destination axis may have zero scale only when the
source coordinate is constant over the complete declared pcurve interval.
Distinct source and destination endpoint values still use an affine map. A
source axis with equal endpoints but interior variation, or a varying source
axis mapped to equal destination endpoints, rejects the synthesized variant;
the pcurve remains opaque rather than losing its locus. Coordinate bounds use
33 samples over the declared interval, including both endpoints.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PC-03. Surface chart remap from the pcurve population

**Question.** What STEP rule establishes surface chart remap from the pcurve population?

**Known.** A pcurve uses the parameterization of its owning surface. The
surface population does not define or modify that chart. A linear extrusion
inherits the directrix parameter as `u`; a revolution inherits the directrix
parameter as `v` and uses the surface angle as `u`. A non-linear directrix
therefore keeps its native parameterization. The reader does not infer a
surface-wide affine map from trimmed pcurves. A bounded procedural pcurve may
still receive a use-scoped endpoint calibration under PC-02.

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

### PC-05. Periodic trim interval

**Question.** What STEP rule establishes periodic trim interval?

**Known.** A cyclic trim follows the directed parameter branch. For a
forward sense, if the second select is below the first, add one basis period
to the second select. For a reversed sense, if the first select is below the
second, add one basis period to the first select. The stored local domain is
the absolute directed span after that adjustment. Non-cyclic trims use the
stored selects without adjustment. The same rule applies independently to
the U and V axes of `RECTANGULAR_TRIMMED_SURFACE`.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PC-06. Default placement reference direction

**Question.** What STEP rule establishes default placement reference direction?

**Known.** An omitted or parallel `AXIS2_PLACEMENT_3D.ref_direction` uses
the projection of global +X onto the plane normal to the axis. When the axis
is within `1e-12` of parallel to X, it uses global +Y before projection. The
STEP reader applies this rule locally, so a neutral stability helper cannot
change STEP chart semantics.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PC-07. Ellipse semi-axis canonicalization

**Question.** What STEP rule establishes ellipse semi-axis canonicalization?

**Known.** The IR keeps `major_radius ≥ minor_radius`. For
`semi_axis_1 < semi_axis_2`, it stores `cross(axis, ref_direction)` as the
major direction and maps the source parameter with `v = u − π/2`. Numeric
`TRIMMED_CURVE` selectors apply that phase after angular unit conversion;
Cartesian selectors invert the canonical geometry directly. Replicas, nested
trims, and spatial offsets inherit the phase.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### BR-01. Topology root identity

**Question.** What STEP rule establishes topology root identity?

**Known.** The topology-root cache key includes the governing root type,
the resolved shell identities, and shell orientations. Multiple
representations that reach one root of the same type reuse its committed body
identity. Distinct root records retain distinct bodies when their root types
differ, even when they share shell carriers. Body kind is therefore
independent of instance-number order.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### BR-02. Outer and void shell roles

**Question.** What STEP rule establishes outer and void shell roles?

**Known.** `BREP_WITH_VOIDS` attribute 1 is the outer shell and attribute 2
contains the void shells. The IR stores the outer role in the first
`Region.shells` entry. The reader rejects a solid root when the outer shell
splits into multiple connected components, because the extra component cannot
retain the outer role in the current IR. Sheet and general roots still retain
each valid connected component.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PS-01. Placement binding for repeated child uses

**Question.** What STEP rule establishes placement binding for repeated child uses?

**Known.** A parent representation's mapped-item order does not bind
repeated uses of one child definition to individual
`NEXT_ASSEMBLY_USAGE_OCCURRENCE` records.

`step.md` §8 "Repeated child uses without an occurrence-specific
shape representation remain ambiguous and report the unresolved placement."
settles this as unresolvable.

The decoder may infer a parent-representation placement only when
each child definition occurs once in that parent's usage set and the complete
mapped-child sequence agrees with the usage set. Repeated child uses require
an occurrence-owned shape representation or an explicit context-dependent
placement. Without one, the occurrence keeps identity transform and reports
`AssemblyPlacementsNotTransferred`.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PS-02. Transform direction of `ITEM_DEFINED_TRANSFORMATION`

**Question.** Which of `transform_item_1` and `transform_item_2` is expressed
in the component frame?

**Known.** `transform_item_1` belongs to `rep_1` and `transform_item_2`
belongs to `rep_2`. For an occurrence, the reader identifies the child and
parent representation sets from the usage definitions. An endpoint belongs to
a set when it is a member of that set or is connected to a member by one or
more parameterized `SHAPE_REPRESENTATION_RELATIONSHIP` edges. Those edges are
undirected for representation identity matching. An empty inherited subtype
partial contributes no edge. The reader maps item 1 to item 2 when `rep_1` is
the child and `rep_2` is the parent; it maps item 2 to item 1 when the
relationship endpoints are reversed. An endpoint pair that matches neither
order, or matches both orders, leaves the occurrence placement unresolved and
reports `AssemblyPlacementsNotTransferred`.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PS-03. Repeated mapped placements of one representation

**Question.** What STEP rule establishes repeated mapped placements of one representation?

**Known.** A body-producing representation may have several standalone
`MAPPED_ITEM` records only when all records resolve to one transform. Distinct
placements cannot be represented by one `Body.transform`; the reader leaves
that body unplaced and reports `AssemblyPlacementsNotTransferred`. Mappings
owned by product occurrences use occurrence transforms and are not part of
this rule.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PS-04. Product and product-definition identity

**Question.** What STEP rule establishes product and product-definition identity?

**Known.** A CADIR product definition represents one STEP
`PRODUCT_DEFINITION` view. A product with one definition keeps the historical
`step:product:product#<product>` identity. When one `PRODUCT` has multiple
definitions, each view receives a distinct deterministic identity suffixed by
its definition instance. Shape bodies and definition descriptions bind to
their own view; they are not merged. Each definition not named as a usage
receives one root occurrence, and every usage occurrence references the
specific child definition view. When a presentation layer references the
source `PRODUCT`, the reader emits all of that product's definition views, but
the source rule for their order is not established.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: reopened after reviewing closing commits `dd0761ab4` and `225b25fb6`. `dd0761ab4` changed `crates/cadmpeg-codec-step/src/reader/product.rs:72-88` to sort definitions by `RawRecord.span.start`; `step.md` §8 then states that byte order as “source-definition order.” The reordered test in `crates/cadmpeg-codec-step/src/reader/presentation/tests.rs:71-106` was authored with that implementation and proves only that the policy is applied. The presentation assignment carries a set of items, and no ordered source attribute tying a `PRODUCT` to its definitions was identified. If two otherwise equivalent definitions are reserialized with their DATA records swapped, the emitted `PresentationItem` order changes while the source relationships do not. Native-reference assertions do not establish the order or the rule that every definition view must be expanded. Keep this item open until the ISO 10303 part text or an exporter-authored witness file settles view identity and order.

### PS-05. Mapped-item scope for occurrence placement

**Question.** Must a `MAPPED_ITEM` that supplies an occurrence placement
belong to the parent's own representation?

**Known.** An inferred occurrence placement must be a mapped item directly
listed by a representation of the occurrence's parent definition. The reader
ignores mapped items listed by unrelated representations. If no scoped mapping
remains, the occurrence keeps identity placement and reports
`AssemblyPlacementsNotTransferred`. Occurrence-owned shape representations and
the complete parent-representation sequence inference are evaluated before
this fallback.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### PS-06. Validation representation item count

**Question.** How many measure items may one geometric-validation
representation carry?

**Known.** A validation representation transfers every referenced item.
Area, volume, and centroid items are evaluated independently. An unsupported
item reports a warning naming that item and does not suppress other items in
the same representation. Repeated item references are evaluated once.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### AP-01. Datum identification for a complex datum

**Question.** What STEP rule establishes datum identification for a complex datum?

**Known.** The `DATUM` partial supplies the `identification` attribute of
a complex `DATUM` instance. The inherited `SHAPE_ASPECT` partial supplies its
name, target, and product shape.

`RecordExt::parameters` returns the parameters of the first partial
only (`crates/cadmpeg-codec-step/src/reader/pmi.rs:1274-1279`). The datum
reader scans those parameters for the identification text and substitutes the
synthetic string `#<id>` when it finds none (`pmi.rs:59-73`). Part 21 orders
complex partials alphabetically, and the parser enforces that order.

The reader looks up datum identification by partial name instead of
using the first complex partial. A synthesized complex datum with an empty
`COMMON_DATUM` partial retains identification `A` and its inherited
`SHAPE_ASPECT` target.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### AP-02. Dimension nominal value selection

**Question.** What STEP rule establishes dimension nominal value selection?

**Known.** The characteristic representation collects all reachable
measure representation items from its item aggregate. A unique item named
`nominal value` supplies the nominal. Without that name, exactly one measure
item supplies it. Multiple unnamed items remain ambiguous, produce a metadata
warning, and do not select a source-order value.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### AP-03. Geometric tolerance kind selection

**Question.** What STEP rule establishes geometric tolerance kind selection?

**Known.** A complex geometric tolerance takes its kind from the exact
geometric-tolerance leaf partial. Inherited base and modifier partials do not
select the kind. The reader uses the same exact leaf table for direct and
complex instances, so a non-leaf name that ends in `_TOLERANCE` remains an
opaque source record instead of changing the leaf kind. The writer emits each
supported leaf entity by its corresponding IR kind.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### AP-04. Annotation text completeness

**Question.** What STEP rule establishes annotation text completeness?

**Known.** A direct text carrier or a graph with exactly one reachable text
carrier supplies the presentation text. A graph with multiple reachable text
carriers has no ordered composition in this model, so the text remains absent,
a metadata loss is emitted, and every carrier remains a named opaque record
with its source links. The reader never selects a carrier by traversal or
serialization order.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### AP-05. Style precedence for independent styled items

**Question.** What STEP rule establishes style precedence for independent styled items?

**Known.** Override chains remove their overridden base styles. Independent
effective styles all remain appearance bindings. The reader sets a neutral
face or body scalar color only when those styles produce one distinct color;
duplicate colors collapse to that value. Conflicting colors leave the scalar
unset and emit a `MetadataNotTransferred` loss naming every contributing
styled item. Source instance order never selects a scalar color.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### AP-06. Surface style side selection

**Question.** What STEP rule establishes surface style side selection?

**Known.** `SURFACE_STYLE_USAGE` applies its style to the side named by its
`side` enumeration: `.POSITIVE.` is the surface-normal side, `.NEGATIVE.` is
the opposite side, and `.BOTH.` applies to both sides. CADIR stores one neutral
surface color, so the reader selects `.BOTH.` before `.POSITIVE.` before
`.NEGATIVE.` independently of aggregate serialization order.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### AP-07. Triangle strip winding

**Question.** What STEP rule establishes triangle strip winding?

**Known.** A strip with indices `v[0]` through `v[n]` produces
`[v[i], v[i+1], v[i+2]]` for an even `i` and
`[v[i+1], v[i], v[i+2]]` for an odd `i`. Fans keep their first index and
advance the other two. The reader applies this rule and the regression covers
the first two triangles of one strip.

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

### EL-04. Signature section boundary

**Question.** What STEP rule establishes signature section boundary?

**Known.** Signature sections follow `END-ISO-10303-21;`. Each section
starts with `SIGNATURE;` and ends at its own token `ENDSEC;`. The decoder
retains every complete section range in source order.

`step.md` §2 and §7 define the post-terminator placement and the
base64 content. The lexer finds the first token-boundary `ENDSEC;` after each
`SIGNATURE;`; it does not search for the exchange terminator or merge adjacent
signature sections.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### EL-05. Schema identifier interpretation

**Question.** What STEP rule establishes schema identifier interpretation?

**Known.** `FILE_SCHEMA` contains one or more unique string identifiers.
The first identifier governs the application protocol and edition. An
identifier is a schema name with an optional brace-delimited object identifier
whose components are space-separated signed decimal integers. The decoder selects
AP242 edition 1, 2, or 3 only for the exact long-form name and exact object
identifiers `1 0 10303 442 1 1 4`, `1 0 10303 442 3 1 4`, and
`1 0 10303 442 4 1 4`. Other AP242 object identifiers report an unspecified
edition. Leading and trailing whitespace around an identifier is ignored.
Later identifiers remain metadata and do not change the selection.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### EL-06. Omitted entity name repair and anchor order

**Question.** What STEP rule establishes omitted entity name repair and anchor order?

**Known.** Omitted inherited `name` repair runs after edition-3 anchor
values resolve.

The parser holds a table of about 100 entity names and inserts an
empty name when a single-partial record of one of those names has a first
parameter that is neither a string nor omitted
(`crates/cadmpeg-codec-step/src/parse.rs:323-435`, `:671-675`). The repair
has its own diagnostic, produces a `NoncanonicalSourceSyntax` loss with byte
provenance, and is rejected by strict decode. `step.md` states no attribute
layout for these entities and this ledger has no item for the repair.

The parser reads each raw record, resolves all anchors in anchor
values and record parameters, then tests the first parameter of each single
named carrier against the carrier's inherited `name` slot. A resource anchor
that resolves to a string is therefore a real name and does not trigger repair.
The existing carrier table remains the scope of repair; other entity layouts
are not shifted.

**Need.** We need the primary format rule from the ISO 10303 part text and a reordered or malformed witness input that verifies this behavior.

**Note.** QA audit: this item was removed by 74d66189d during a bulk refactor and open-item cleanup. The removal did not include an item-specific evidence change for this rule. Reopen until the current specification and implementation are tied to primary format evidence and an adversarial fixture.

### AP-08. Context-dependent style selection

**Question.** How does a `STYLED_ITEM` select among valid
`PRESENTATION_STYLE_BY_CONTEXT` assignments, and what neutral appearance is
retained when the requested presentation context is unavailable?

**Known.** A `STYLED_ITEM` may carry multiple
`PRESENTATION_STYLE_BY_CONTEXT` assignments. The reader transfers a branch
only when its context directly contains the styled item
(`crates/cadmpeg-codec-step/src/reader/presentation.rs:271-318`); an unresolved
branch remains source-native and records
`presentation.context-dependent-style-unresolved`
(`presentation.rs:319-335`). `find_color` excludes context-style records
(`presentation.rs:1045-1047`). The neutral model still has no
representation-context identity for choosing among multiple matching branches.

**Note.** The direct membership test refuses a branch whose context does not
contain the styled item. Multiple matching branches are flattened into the
same neutral style path, but the model has no context identity to keep those
bindings separate. AP-05 and AP-06 settle independent styled-item conflicts
and surface-side precedence; they do not settle this projection.

**Need.** We need the context matching and precedence rule, a policy for
preserving separate context bindings or reporting ambiguity, and a witness
file with two valid context styles consumed by different presentation
contexts.

QA audit: reopened after reviewing closing commit f3f7437dc. The closing commit records a conservative no-context transfer policy and a synthetic regression. It does not identify the source presentation context or establish a neutral representation-context projection.

### AP-09. Multiple same-domain style assignments

**Question.** How does one STYLED_ITEM select among multiple valid same-domain style assignments?

**Known.** `crates/cadmpeg-codec-step/src/reader/presentation.rs:271-350`
flattens style references and resolves them through `find_color`. The
combiner at `presentation.rs:1083-1127` ranks candidates by surface-side rank
and alpha. Distinct colors at equal rank and equal alpha become an ambiguity,
and the caller records `ConflictingScalarColors` without selecting a scalar
color. Distinct colors at equal side rank but different alpha still use alpha
as a tiebreak. The specification says source order does not select a scalar
color, but it does not establish this alpha precedence or a complete rule for
conflicting assignments within one style graph.

**Need.** We need a witness file with two same-domain, equal-precedence colors on one styled item and a reordered copy, plus the source rule for conflicting assignments. The file must also cover same-rank colors with distinct transparency to establish whether alpha can rank colors.

**Note.** QA audit: reopened after reviewing closing commit `857867aaf`. The
closing code removes first-candidate selection only for equal side rank and
equal alpha, and the test at `crates/cadmpeg-codec-step/src/reader/presentation/tests.rs:508-534`
is authored with that code. A conservative ambiguity loss is not evidence of
the source precedence rule. If two valid same-domain renderings have the same
surface side but different transparency and distinct colors, `ColorResolution::priority`
selects the lower alpha before conflict detection; no STEP rule in the current
specification establishes that hue selection. The equal-alpha case now refuses
and records a loss, but the full assignment question remains open.

### UM-04. Document fallback unit identity

**Question.** Which unit context and unit occurrence supply the document
fallback when a representation has no usable `GLOBAL_UNIT_ASSIGNED_CONTEXT`?

**Known.** Representation-local unit scopes are resolved from their reachable
representation closure. `document_unit_scale` at
`crates/cadmpeg-codec-step/src/reader/geometry.rs:3133-3180` collects every
matching global context and returns a scale only when all resolved scales are
equal. If no context supplies the dimension, it applies the same uniqueness
rule to the remaining unit records. Conflicting values return no fallback;
the code does not identify which context owns an unscoped document scale.

**Note.** UM-01 settles scoped representation units but does not settle this
fallback identity. The later implementation removes record-order selection by
requiring one common scale, but that refusal rule does not establish the STEP
document unit when contexts disagree or no context is linked.

**Need.** We need the STEP document-level unit ownership rule and witness
files with multiple global contexts, including conflicting and equivalent
contexts, before selecting a fallback scale.

QA audit: reopened after reviewing closing commit 706e743d3. The closing commit makes the fallback aggregation order-independent, but uniqueness across all global contexts is still a decoder policy and does not establish document-level unit ownership.

### TP-10. Malformed duplicate outer-bound fallback

**Question.** When malformed input gives one face more than one
`FACE_OUTER_BOUND`, which loop role and implicit face carrier should the
decoder retain?

**Known.** ISO 10303-42 permits at most one `FACE_OUTER_BOUND` for a face.
The current reader counts outer bounds before assigning a role. If more than
one exists, it records `face.multiple-outer-bounds`, omits the containing
topology shell, and does not derive an implicit face carrier
(`crates/cadmpeg-codec-step/src/reader/topology.rs:1989-2007`).

**Note.** This is a conservative malformed-input refusal, not evidence that
STEP prescribes omission of the whole topology shell. TP-04 was closed because
conforming STEP prohibits multiple outer bounds; that closure does not
establish this malformed-input disposition.

**Need.** We need an explicit conservative salvage policy, recorded as a
CADIR decision, or evidence that first-role retention is required, with
reordered duplicate-outer fixtures and validation results.

QA audit: reopened after reviewing closing commit 62e044540. The closing commit rejects a malformed duplicate outer-bound case. This is a conservative refusal policy, not evidence that the format requires this salvage disposition.

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

### PS-07. Duplicate context-dependent occurrence placement

**Question.** What is the disposition when more than one CONTEXT_DEPENDENT_SHAPE_REPRESENTATION resolves to one NEXT_ASSEMBLY_USAGE_OCCURRENCE?

**Known.** crates/cadmpeg-codec-step/src/reader/product.rs:797-809 iterates every context-dependent relation and stores its transform with result.insert(usage, transform). A later relation replaces an earlier one. The iteration follows exchange.entities record order. No ambiguity loss records the replacement.

**Need.** We need the uniqueness rule for occurrence placement relations, and a witness file with two valid relations for one usage, including a record-order permutation.

**Note.** Reordering the same two relations can change the occurrence transform without changing the represented usage. The current code silently selects one candidate.

### AP-10. Multiple surface transparency properties

**Question.** How does a surface style select transparency when one
`SURFACE_STYLE_RENDERING_WITH_PROPERTIES` references multiple valid
`SURFACE_STYLE_TRANSPARENT` properties?

**Known.** `crates/cadmpeg-codec-step/src/reader/presentation.rs:1212-1228`
walks the property references and returns the first finite transparency. No
conflict loss records distinct transparency values, and the specification
defines alpha conversion but no precedence for multiple properties.

**Need.** We need a witness file with two distinct transparency properties
under one rendering record and a reordered copy, plus the source uniqueness or
precedence rule.

**Note.** Reordering the property references can change appearance alpha while
the color and both transparency properties remain valid. The existing
transparency test uses separate rendering records and does not exercise this
selection.

### TS-01. Invalid repositioned tessellation placement

**Question.** What happens when a REPOSITIONED_TESSELLATED_ITEM has a missing, invalid, or unknown placement reference?

**Known.** crates/cadmpeg-codec-step/src/reader/tessellation.rs:456-459 treats repositioned_placement returning None as if the wrapper supplied no local placement. repositioned_placement at tessellation.rs:537-545 returns None for a missing reference or placement entry. The specification defines valid repositioning and conflicting-placement refusal, but no invalid-single-placement disposition is recorded.

**Need.** We need a malformed wrapper file with one invalid placement reference and an expected loss or source-native disposition that prevents coordinate substitution.

**Note.** A tessellated leaf below a malformed repositioning wrapper can be emitted at the inherited or identity placement with no loss. The decoded coordinates then look valid but are silently in the wrong frame.
