# STEP Open Items

This document lists the parts of STEP exchange formats that we do not know. The specification `step.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. External resources

### ER-01. URI resolution

**Question.** Which base URI and normalization rules apply to each relative URI in a REFERENCE section or a document-reference entity?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." state that a REFERENCE entry binds a local resource name to a resource URI and that a target outside the exchange structure is an external dependency.

**Need.** We must know the rules to identify the external resource that a relative URI selects.

### ER-02. Resource access

**Question.** Which retrieval and authentication procedure applies to each external resource URI?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." identifies URI targets outside the exchange structure as external dependencies. The clear-text exchange structure does not contain an access procedure.

**Need.** We must know the procedure to obtain the selected external resource.

### ER-03. Resource composition

**Question.** How does each external resource combine with the local instance graph?

**Known.** `step.md` §5 "Instance names share one namespace across all DATA sections." through `step.md` §5 "Instance names share one namespace across all DATA sections." define identity and reference resolution inside the DATA sections. `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." define local resource bindings and external dependencies.

**Need.** We must know the composition rule to resolve cross-resource identities and build one product graph.

### ER-04. Resource cache identity

**Question.** Which URI components and resource metadata determine whether two external resource references identify the same cached resource?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." and `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." state that each REFERENCE entry contains a resource URI. The specification gives no cache-identity rule.

**Need.** We must know the identity rule to reuse a retrieved resource without combining different resources.

## 2. AP242 BO-Model sidecars

### BM-01. Sidecar envelope

**Question.** What XML grammar and file relationship identify an AP242 BO-Model sidecar?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" identify an AP242 BO-Model XML sidecar as an encoding that is distinct from the Part 21 clear-text exchange structure.

**Need.** We must know the envelope to detect, parse, and associate the sidecar with its Part 21 exchange structure.

### BM-02. Sidecar composition

**Question.** How do AP242 BO-Model XML identities and values combine with the Part 21 instance graph?

**Known.** `step.md` §5 "Instance names share one namespace across all DATA sections." through `step.md` §5 "Instance names share one namespace across all DATA sections." define identity and reference resolution inside the Part 21 DATA sections. The specification gives no cross-encoding composition rule.

**Need.** We must know the composition rule to build one product graph from the Part 21 exchange structure and its sidecar.

## 3. Containers and other encodings

### CE-01. ZIP container layout

**Question.** Which ZIP entries, names, metadata, and relationships form an edition-3 exchange container?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" identify a ZIP container as distinct from a clear-text Part 21 exchange structure. `step.md` §2 "A clear-text exchange structure uses this outer grammar:" defines the clear-text outer grammar.

**Need.** We must know the layout to locate and identify each exchange resource in the container.

### CE-02. ZIP resource composition

**Question.** How do references between exchange resources in an edition-3 ZIP container resolve?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." define resource names and URIs in a Part 21 REFERENCE section. The specification gives no container-relative resolution rule.

**Need.** We must know the resolution rule to combine the contained resources into one product graph.

### CE-03. Part 28 XML grammar

**Question.** What XML grammar represents an AP203, AP214, or AP242 exchange structure in Part 28?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" define Part 21 clear text and identify Part 28 XML as a distinct encoding.

**Need.** We must know the grammar to parse record boundaries, values, and references from Part 28 XML.

### CE-04. Part 28 graph mapping

**Question.** How does each Part 28 XML construct map to the entity graph and invariants in `step.md`?

**Known.** `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," through `step.md` §5 "Instance names share one namespace across all DATA sections." define Part 21 values, records, identities, and references. The specification gives no Part 28 mapping.

**Need.** We must know the mapping to apply schema decoding to a Part 28 exchange structure.

### CE-05. Part 26 binary grammar

**Question.** What HDF5 layout represents an AP203, AP214, or AP242 exchange structure in Part 26?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" define Part 21 clear text and identify Part 26 binary or HDF5 as a distinct encoding.

**Need.** We must know the layout to parse record boundaries, values, and references from Part 26 data.

### CE-06. Part 26 graph mapping

**Question.** How does each Part 26 HDF5 construct map to the entity graph and invariants in `step.md`?

**Known.** `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," through `step.md` §5 "Instance names share one namespace across all DATA sections." define Part 21 values, records, identities, and references. The specification gives no Part 26 mapping.

**Need.** We must know the mapping to apply schema decoding to a Part 26 exchange structure.

## 4. User-defined names

### UD-01. User-defined entity semantics

**Question.** What entity semantics does each user-defined `!` entity name select?

**Known.** `step.md` §3 "user_name" defines the syntax of a user-defined name. `step.md` §5 "Instance names share one namespace across all DATA sections." through `step.md` §5 "Instance names share one namespace across all DATA sections." require an unknown entity to retain its name, complete spans, and links to other named opaque records.

**Need.** We must know the semantics to transfer a user-defined entity to typed native or neutral records.

### UD-02. User-defined type semantics

**Question.** What value semantics does each user-defined `!` type name select?

**Known.** `step.md` §3 "user_name" defines the syntax of a user-defined name. `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," through `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," define a typed parameter as a name with one parameter.

**Need.** We must know the semantics to decode the wrapped parameter as a typed value.

## 5. Signatures

### SG-01. Signature method selection

**Question.** Which SIGNATURE field identifies the signature method and its parameters?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." define the complete byte boundary of a SIGNATURE section. The specification gives no field grammar for its content.

**Need.** We must know the selection rule to choose the correct signature verification method.

### SG-02. Signed byte sequence

**Question.** Which exact bytes does each signature method authenticate?

**Known.** `step.md` §2 "A clear-text exchange structure uses this outer grammar:" through `step.md` §2 "A clear-text exchange structure uses this outer grammar:" place the optional SIGNATURE section after all DATA sections and before the exchange terminator. `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." define the byte boundary of the SIGNATURE section.

**Need.** We must know the byte sequence to calculate the verification input.

### SG-03. Signature value encoding

**Question.** How does each signature method encode its signature value and verification material in the SIGNATURE section?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." require retention of the complete SIGNATURE byte range. The specification gives no field grammar for the retained content.

**Need.** We must know the encoding to extract the signature value, keys, certificates, and method parameters.

### SG-04. Signature verification result

**Question.** Which validation conditions make each signature valid, invalid, or indeterminate?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." define structural retention only. The specification gives no cryptographic validation conditions.

**Need.** We must know the conditions to report a signature verification result.

## 6. Topology and pcurve decisions

### TP-01. Shared-edge ownership

**Question.** When one STEP edge or vertex is referenced by multiple independent
shell owners, should CADIR preserve one shared identity, create one occurrence
identity per owner, or reject the conflicting topology?

**Known.** The decoder uses owner-scoped edge and vertex identities when the
source ownership is ambiguous. It keeps the per-occurrence rule and reports an
identity collision when two committed drafts still claim the same destination
identity.

**Need.** We need a standards-valid shared-edge construction and its ownership
semantics before changing the current rule.

### TP-02. Seam pcurve selection

**Question.** Which same-surface pcurve belongs to each seam coedge when a seam
curve carries more than one candidate pcurve?

**Known.** The source curve can carry multiple pcurves for one surface. The
decoder maps each candidate through the owning surface and selects a candidate
when its endpoint fit is uniquely continuous within the topology tolerance. A
tie between candidates with equivalent model-space loci is one semantic
carrier; the decoder retains the first source candidate. Distinct tied or
unresolved candidates remain detached from the coedge and produce a topology
loss.

**Need.** Endpoint continuity does not distinguish distinct seam branches with
the same endpoints. We need the standards-valid UV branch and orientation rule
for selecting one of those tied candidates. Serialized occurrence order is
not a sufficient rule.

### TP-05. Partial solid and tolerant point carriers

**Question.** Should CADIR gain a tolerant point carrier or a partial-solid
representation for a solid with one missing mandatory vertex point?

**Known.** Solid roots commit atomically. A missing mandatory point rejects the
complete solid and reports the failed STEP carrier.

**Need.** We need measured loss rates and an IR design before changing the
atomic-solid invariant.

### TP-06. Implicit face-plane orientation

**Resolved.** A base `FACE` without a surface uses the first outer boundary,
or the first valid boundary when no outer role exists. Its signed ring area
defines the normal. The centroid defines the origin, and the projection of the
most orthogonal global coordinate axis defines the u-axis, with x/y/z tie
order. The ring must be planar within the document coincidence tolerance and
`1e-12` of its scale. Degenerate or non-planar rings reject the topology root;
an `ORIENTED_FACE` still composes reversal through the face sense and boundary
traversal. This makes the inferred carrier independent of cyclic ring
serialization and prevents a non-planar boundary from receiving a fabricated
plane.

### TP-07. Pcurve recursion and normalization

**Question.** What normalization and recursion guard rules apply to cyclic
2D curve definitions, 2D `LINE` carriers, and complex `PCURVE` entities?

**Known.** Supported 2D carriers decode as typed pcurves. Unsupported or cyclic
carriers remain opaque.

**Need.** We need fixtures for cycle handling, 2D line normalization, and
complex `PCURVE` support before extending the typed domain.

### TP-08. Face-bound partial dispatch

**Resolved.** The partial with the boundary parameters supplies the inherited
`FACE_BOUND` attributes. An empty `FACE_OUTER_BOUND` partial supplies the
outer-role classification.

**Known.** `has_type` matches a partial name exactly and does not walk the
EXPRESS subtype hierarchy
(`crates/cadmpeg-codec-step/src/reader/topology.rs:4633`). Two sites choose
the governing partial in opposite orders. The shell reader tries `FACE_BOUND`
first (`topology.rs:2205`). The implicit-plane reader tries `FACE_OUTER_BOUND`
first (`topology.rs:2978`). `FACE_OUTER_BOUND` adds no attributes to
`FACE_BOUND`, so the second site reads attribute 1 of an empty partial and
returns no loop.

**Rule.** Face-bound classification reads the presence of
`FACE_OUTER_BOUND`, while attribute lookup selects the first face-bound
partial that carries the three boundary parameters. The shell reader and
implicit-plane reader use this same dispatch. The synthesized complex-face
fixture covers the inherited-attribute form.

## 7. Units and measures

### UM-01. Unit context selection

**Question.** Which `GLOBAL_UNIT_ASSIGNED_CONTEXT` supplies the length and
plane-angle scale for a value, when an exchange structure contains more than
one context?

**Known.** `step.md` §8 "Length values convert to millimetres." gives the
target units. `step.md` §8 "Representation uncertainty is a linear tolerance
measured in the representation's length unit." states a per-representation
model. The decoder resolves one document-global scale instead. It takes the
context that `BTreeMap` iteration reaches first, which is the context with the
lowest instance name, and does not compare the other contexts. When that
context lists no length unit, it adopts any `LENGTH_UNIT` record anywhere in
the file (`crates/cadmpeg-codec-step/src/reader/geometry.rs:2635-2661`).
`plane_angle_scale` uses the same rule (`geometry.rs:2663-2689`). The scale
applies to every point, radius, and extent, and passes into `pmi`,
`tessellation`, `topology`, and `validation`.

**Need.** An assembly whose imported component declares an inch context and
whose top level declares a millimetre context scales every coordinate in the
file by the lower-numbered context. No loss is recorded, because the
`unresolved_unit_loss` path fires only when resolution returns nothing, never
when it resolves to the wrong context. We must know which context governs a
value to bind a scale per representation.

### UM-02. Representation uncertainty selection

**Resolved.** The linear tolerance is the `UNCERTAINTY_MEASURE_WITH_UNIT`
whose unit resolves to a length unit. If several length measures are present,
the measure named `distance_accuracy_value` takes precedence. Without that
name, exactly one length measure is required. An angular measure does not
block a later length measure, and an ambiguous set produces a machine-readable
geometry loss instead of selecting by source order.

**Known.** `step.md` §8 defines the length-unit invariant and selection rule.

### UM-03. SI prefix on plane-angle units

**Resolved.** An SI prefix applies to a plane-angle `SI_UNIT` before the
unit is converted to radians. An omitted prefix has factor 1.

**Known.** `step.md` §8 "SI prefixes apply before conversion-based-unit
factors." states the rule without restriction to a unit kind.

**Rule.** The angular unit resolver reads the optional prefix in parameter 0,
uses the same SI prefix factors as the length resolver, and multiplies the
resulting factor into conversion-based-unit factors. The rule is covered by
the parser-level `MILLI` and omitted-prefix regression cases in
`reader/geometry.rs`.

## 8. Parameter charts and unit repair

### PC-01. Angular parameter unit repair

**Question.** May a pcurve parameter axis use a plane-angle unit other than
the unit the representation context declares?

**Known.** `step.md` §8 "Length values convert to millimetres." and `step.md`
§8 "A cylinder or cone uses plane-angle scale for `u`" bind the axis to the
declared unit. The decoder does not hold to that binding. It builds a
candidate set of the declared scale and the degree or radian alternative,
maps each candidate through the owning surface, and scores each by the
distance from the mapped surface point to the edge endpoint
(`crates/cadmpeg-codec-step/src/reader/geometry.rs:1742-1820`). It accepts
the alternative when the best score is at most 1.0 tolerance, the declared
score is more than 10.0 tolerances, and the declared score is more than 100
times the best. The same rule exists for revolution surfaces
(`crates/cadmpeg-codec-step/src/reader/topology.rs:3446-3463`).

**Need.** The thresholds 1.0, 10.0, and 100.0 have no source. Only the
endpoint parameters are observed; the curve interior is never sampled, and a
coedge with more than one pcurve contributes one endpoint per pcurve. A
pcurve that is correct under the declared unit but whose edge fails the fit
for an unrelated reason can therefore be rescaled by π/180 across its whole
domain. The result is a free-text warning, not a `LossNote`, so it does not
appear in the decode report losses. We must know whether producers emit
degree-valued pcurve axes, and under which declaration, before a numeric fit
may override a declared unit.

### PC-02. Synthesized pcurve chart

**Question.** May the decoder replace a pcurve's parameterization with an
affine map that it derives from the edge endpoints?

**Known.** `step.md` §8 states the opposite twice: "A pcurve on either surface
maps into the same U/V parameterization as its owning surface" and "A pcurve
carrier that cannot preserve its native parameterization under an anisotropic
surface-unit map remains opaque." The decoder instead inverts the surface at
the edge's two vertex points, builds a per-axis affine map from the pcurve
domain endpoints onto those surface parameters, and accepts the variant with
the best endpoint score
(`crates/cadmpeg-codec-step/src/reader/topology.rs:3218-3322`). The score is
the larger of the two endpoint distances (`topology.rs:3841`). The 33-sample
locus comparison is used only to break ties between source candidates, never
to check a synthesized chart against the source curve.

**Need.** When a pcurve's two domain endpoints share one axis value, the
degenerate branch sets that axis scale to 0.0 and its offset to the endpoint
value (`topology.rs:3306-3313`), so the axis becomes constant and a bowed
curve collapses to an isoparametric line. Both endpoints still fit, so the
variant is selected and written back. No loss is recorded. We must know
whether producers parameterize a bounded edge locally, as the code comment
asserts, before the decoder may synthesize a chart rather than stay opaque.

### PC-03. Surface chart remap from the pcurve population

**Question.** May the aggregate coordinate bounds of the pcurves on a surface
define that surface's source chart?

**Known.** For a procedural surface the decoder collects the coordinate bounds
of every pcurve on it, selects the isoparametric pcurve with the largest
varying span whose other axis is constant within 1e-12 relative
(`crates/cadmpeg-codec-step/src/reader/topology.rs:3571-3592`), and builds a
linear rescale from that span onto the surface domain, together with a
direction-reversing variant (`topology.rs:3535-3569`). Both are offered to
the endpoint score.

**Need.** The rule assumes the observed pcurves span the whole source chart
and that the source-to-surface map is linear. A face trimmed so that no
pcurve spans the full directrix yields a short span and stretches every
pcurve on the surface. A non-uniformly parameterized NURBS directrix does not
map linearly from the producer's parameterization to the knot parameter, so
the endpoints fit and every interior point is wrong. Nothing detects either
case and no loss is recorded. We must know the real source parameterization
rule for procedural surfaces.

### PC-04. Chart write-back to the shared pcurve

**Question.** Which coedge owns the parameterization of a pcurve that several
edges reference?

**Known.** `select_associated_pcurve` runs for each coedge with that coedge's
vertex points and overwrites the pcurve arena entry with the variant
calibrated to those points
(`crates/cadmpeg-codec-step/src/reader/topology.rs:3806-3817`). There is no
guard against a previous write.

**Need.** Two `EDGE_CURVE` records on one `SURFACE_CURVE`, or a `SUBEDGE` that
inherits its parent's pcurve, calibrate the same arena entry in turn. The
first coedge stores a `parameter_range` measured in the chart that the second
coedge then replaces, so the stored range no longer refers to the stored
geometry. We must know whether a pcurve chart is a property of the carrier or
of the occurrence.

### PC-05. Periodic trim interval

**Question.** Does a trim interval on a closed curve or surface wrap across
the parameter seam?

**Known.** `step.md` §8 "Its local U and V domains are `0..abs(u2-u1)` and
`0..abs(v2-v1)`." states an unwrapped rule for
`RECTANGULAR_TRIMMED_SURFACE`. `step.md` §8 "Its local parameter domain is
the directed trim interval measured from the first select" states the same
for `TRIMMED_CURVE`.

**Conflict.** The decoder adds one period to the endpoint before that
subtraction, on axes it finds periodic
(`crates/cadmpeg-codec-step/src/reader/geometry.rs:1074-1086`, `:2847-2871`,
`:3739-3763`). For a cylinder trimmed at `u1 = 5.0`, `u2 = 1.0` with a
positive sense, the decoder gives the domain `0..(1.0 + 2π − 5.0)` and
`step.md` gives `0..4.0`. The two select complementary patches of the
cylinder. The wrap is justified only by code comments. One of the two is
wrong and the item needs a decision.

### PC-06. Default placement reference direction

**Question.** Which reference direction applies to an `AXIS2_PLACEMENT_3D`
whose `ref_direction` is omitted?

**Known.** ISO 10303-42 `first_proj_axis` selects `(1,0,0)`, or `(0,1,0)` when
the axis is parallel to X, and projects it onto the plane normal to the axis.
`step.md` §8 "`CARTESIAN_TRANSFORMATION_OPERATOR_3D` stores a required local
origin and optional axis1, axis2, axis3, and scale attributes." states that
rule for the transformation operator, and `base_axis_3d` implements it
(`crates/cadmpeg-codec-step/src/reader/geometry.rs:4301-4315`). The
specification states no rule for the placement, and the placement uses a
different function: `derive_reference_direction`
(`crates/cadmpeg-ir/src/geometry.rs:332-355`, called at
`crates/cadmpeg-codec-step/src/reader/geometry.rs:213`, `:216`). That function
projects the global basis vector **least** aligned with the axis, and its doc
comment describes a stable direction rather than a format rule.

**Need.** The two rules agree only for a coordinate-aligned axis. For an axis
of `(0.6, 0.8, 0)` the standard gives `(0.8, -0.6, 0)` and the decoder gives
`(0, 0, 1)`. The reference direction is the parameter origin of every circle,
ellipse, and conic on the placement, so every `.PARAMETER.` trim on such a
carrier resolves against a different chart, and every surface built on the
placement gets a different u-origin. Every fixture that omits
`ref_direction` uses an axis of `(0,0,1)`, where the two rules coincide, so no
test separates them. We must confirm the placement default, and decide whether
one shared helper may serve both a stability role and a format rule. The same
helper supplies defaults in the catia, creo, iges, and asm codecs.

### PC-07. Ellipse semi-axis canonicalization

**Question.** May the decoder reorder an `ELLIPSE` semi-axis pair?

**Known.** ISO 10303-42 parameterizes an ellipse as
`center + semi_axis_1·cos(u)·x + semi_axis_2·sin(u)·y`, with `x` the
placement reference direction, and does not require `semi_axis_1` to be the
longer one. When `semi_axis_1` is shorter, the decoder swaps the two radii and
rotates the major direction to `cross(axis, reference_direction)`
(`crates/cadmpeg-codec-step/src/reader/geometry.rs:381-393`).

**Need.** The swap is the substitution `v = u − π/2`, and no compensating
phase shift is applied to trims on that curve. A `TRIMMED_CURVE` with
`.PARAMETER.` selects states its parameters in the source parameterization, so
a trim on a swapped-axis ellipse selects the wrong arc. No fixture combines a
swapped-axis ellipse with a parameter trim, so the reparameterization is never
observed. We must decide whether the IR ellipse carries the source
parameterization or a canonical one, and where the phase shift belongs.

## 9. Body and root identity

### BR-01. Topology root identity

**Question.** Do two topology root records of different kinds that resolve to
the same shell set denote one body or two?

**Known.** `RootKey` holds only the resolved base shells and their
orientations (`crates/cadmpeg-codec-step/src/reader/topology.rs:1789-1791`).
The root record's own entity type is not part of the key, and `BodyKind`
derives from that type (`topology.rs:2023`). Root records are visited in
ascending instance-name order, so the first root builds the body and every
later root with the same shell set is aliased onto it
(`topology.rs:377-401`). `step.md` §8 "Reused source topology roots reuse
their committed body identity." addresses one root reached through several
representations, not two distinct root records.

**Need.** A `CLOSED_SHELL` may be referenced by both a `MANIFOLD_SOLID_BREP`
and a `SHELL_BASED_SURFACE_MODEL`. The solid is then emitted as a sheet body,
or the sheet as a solid, according to which record carries the lower instance
name. Swapping the two instance names swaps the resulting body kind. No loss
is recorded. We must know whether shell sharing between root kinds is valid,
and which root governs the body kind if it is.

### BR-02. Outer and void shell roles

**Question.** How does a region record which of its shells is the outer
boundary?

**Known.** The STEP rule is unambiguous: `BREP_WITH_VOIDS` attribute 1 is the
outer shell and attribute 2 the voids. The reader discards that knowledge and
keeps the role only as position in `region.shells`
(`crates/cadmpeg-codec-step/src/reader/topology.rs:2618-2644`). The writer
recovers it with `split_first`
(`crates/cadmpeg-codec-step/src/lib.rs:1534`). `cadmpeg_ir` documents the
field as "typically one outer, plus voids".

**Need.** Connected-component splitting appends each component of a shell in
place, so an outer shell that decodes into two components inserts an extra
entry ahead of the genuine voids. The writer then exports a piece of the
outer boundary as a reversed void, and moves every void one position later.
No loss is recorded for the role change. We must decide whether the IR
carries an explicit outer-shell role or whether the reader must preserve the
positional contract across component splitting.

## 10. Product structure and assembly

### PS-01. Placement binding for repeated child uses

**Resolved.** A parent representation's mapped-item order does not bind
repeated uses of one child definition to individual
`NEXT_ASSEMBLY_USAGE_OCCURRENCE` records.

**Known.** `step.md` §8 "Repeated child uses without an occurrence-specific
shape representation remain ambiguous and report the unresolved placement."
settles this as unresolvable.

**Rule.** The decoder may infer a parent-representation placement only when
each child definition occurs once in that parent's usage set and the complete
mapped-child sequence agrees with the usage set. Repeated child uses require
an occurrence-owned shape representation or an explicit context-dependent
placement. Without one, the occurrence keeps identity transform and reports
`AssemblyPlacementsNotTransferred`.

### PS-02. Transform direction of `ITEM_DEFINED_TRANSFORMATION`

**Question.** Which of `transform_item_1` and `transform_item_2` is expressed
in the component frame?

**Known.** `step.md` §8 "The two items of an `ITEM_DEFINED_TRANSFORMATION`
belong to the two representations connected by its representation
relationship." states the correspondence but not the direction. The decoder
assumes item 1 is the component and item 2 the assembly, and never reads the
`REPRESENTATION_RELATIONSHIP` `rep_1` and `rep_2` operands
(`crates/cadmpeg-codec-step/src/reader/product.rs:863-892`). The child's
representation set is available at `product.rs:593` and is not consulted.

**Need.** Swapping only the relationship endpoints in a fixture leaves the
child at the same place, where the correspondence rule requires the inverse.
The assumption matches the usual assembly-structure convention, so conforming
files decode correctly, but nothing detects a file that orders its endpoints
the other way and the error is a full mirror of the placement. We must know
whether the direction is fixed by the attribute position or by the
relationship endpoints.

### PS-03. Repeated mapped placements of one representation

**Question.** How many bodies does one shape representation that is mapped at
several placements produce?

**Known.** `step.md` §8 "Mapped representations and context-dependent
relationships that identify one placement apply that placement once."
addresses the single-placement case only. `apply_body_placements` iterates
`MAPPED_ITEM` records in ascending instance-name order and assigns
`body.transform`, replacing any previous value
(`crates/cadmpeg-codec-step/src/reader/product.rs:439-472`).

**Need.** One representation mapped twice, without product structure, yields
one body at the placement of the higher-numbered mapped item. The other
placement is discarded with no warning and no loss. We must know whether
repeated mapped items denote instances, and how the IR represents them.

### PS-04. Product and product-definition identity

**Question.** Does one CADIR product definition correspond to a STEP `PRODUCT`
or to a `PRODUCT_DEFINITION`?

**Known.** The decoder mints part identity from the `PRODUCT` instance and
emits one IR product definition per `PRODUCT`
(`crates/cadmpeg-codec-step/src/reader/product.rs:93-180`, `:941-943`), while
it mints root occurrences per `PRODUCT_DEFINITION` and treats any definition
that no usage names as a root (`product.rs:217-251`).

**Need.** A product with two definitions is emitted once as a part and twice
as an occurrence: once inside the assembly and once as a root instance at the
origin. `shape_bindings` merges both definitions' bodies into the one part
(`product.rs:481-515`), and `definition_descriptions` keeps only the
lowest-numbered definition's description (`product.rs:87`). No loss is
recorded for any of the three. Two assumptions are stacked: that a product
has one meaningful definition, and that "named by no usage" means "assembly
root". We must know the identity rule before either is relied on.

### PS-05. Mapped-item scope for occurrence placement

**Question.** Must a `MAPPED_ITEM` that supplies an occurrence placement
belong to the parent's own representation?

**Known.** The fallback collects every `MAPPED_ITEM` in the exchange
structure and accepts one when the child definition has exactly one usage and
exactly one candidate placement
(`crates/cadmpeg-codec-step/src/reader/product.rs:666-708`). Containment in
the parent representation is never checked. The accepted transform is written
as the occurrence transform and composes down the occurrence tree.

**Need.** A leaf whose representation is mapped only in the root
representation supplies an absolute placement that is then consumed as a
parent-relative one, so the leaf lands at the sum of the two transforms. The
candidate is accepted because it is the only one, not because it was shown to
belong to the parent. We must know the scoping rule for a mapped item that
identifies an occurrence placement.

### PS-06. Validation representation item count

**Question.** How many measure items may one geometric-validation
representation carry?

**Known.** `step.md` §8 "Geometric validation properties read area, volume,
and centroid values through inherited `REPRESENTATION`,
`MEASURE_REPRESENTATION_ITEM`, and `MEASURE_WITH_UNIT` partials" states the
partial chain and not the item count. The decoder reads item 0 of the
representation only
(`crates/cadmpeg-codec-step/src/reader/validation.rs:45-61`).

**Need.** A representation that lists an area item and a volume item reports
the first and drops the second with no diagnostic. The unsupported-value
warning at `validation.rs:126-129` cannot fire for the dropped item, because
that branch needs the first item to fail. We must know whether a validation
representation may carry more than one property.

## 11. Annotation, presentation, and tessellation

### AP-01. Datum identification for a complex datum

**Resolved.** The `DATUM` partial supplies the `identification` attribute of
a complex `DATUM` instance. The inherited `SHAPE_ASPECT` partial supplies its
name, target, and product shape.

**Known.** `RecordExt::parameters` returns the parameters of the first partial
only (`crates/cadmpeg-codec-step/src/reader/pmi.rs:1274-1279`). The datum
reader scans those parameters for the identification text and substitutes the
synthetic string `#<id>` when it finds none (`pmi.rs:59-73`). Part 21 orders
complex partials alphabetically, and the parser enforces that order.

**Rule.** The reader looks up datum identification by partial name instead of
using the first complex partial. A synthesized complex datum with an empty
`COMMON_DATUM` partial retains identification `A` and its inherited
`SHAPE_ASPECT` target.

### AP-02. Dimension nominal value selection

**Question.** Which measure of a dimensional characteristic is the nominal
value?

**Known.** `step.md` §8 "A complex dimension uses its dimensional partial for
its kind and all inherited partials for its name, targets, and characteristic
value." does not say which value when there are several. The decoder collects
every reachable measure in traversal order and takes the first
(`crates/cadmpeg-codec-step/src/reader/pmi.rs:196-199`, `:1053-1084`). It
never reads the measure item's `name`, although the writer emits
`nominal value` and the project's own fixture carries it.

**Need.** A dimension expressed as limits carries `lower limit` and
`upper limit` items and no nominal item. The lower limit becomes the nominal
and the upper limit is dropped with no loss, so a 12.0/12.2 bore reports as
nominal 12.0 with no deviations. We must know the naming or ordering rule
that identifies the nominal value.

### AP-03. Geometric tolerance kind selection

**Question.** Which partial of a complex geometric tolerance names its kind?

**Known.** The decoder takes the first partial other than `GEOMETRIC_TOLERANCE`
whose name maps to a kind (`crates/cadmpeg-codec-step/src/reader/pmi.rs:340-356`),
and the kind table admits any name that ends in `_TOLERANCE`
(`pmi.rs:1008-1031`). Part 21 orders complex partials alphabetically, so the
first matching partial is not the leaf type.

**Need.** A tolerance whose mixin partial also ends in `_TOLERANCE` and sorts
before the leaf is classified by the mixin, so the leaf kind is lost and the
writer drops the tolerance on export. The exclusion list holds only
`GEOMETRIC_TOLERANCE`, and it matches the mixins this project's own writer
emits. We must know the leaf-type rule rather than a name-suffix test.

### AP-04. Annotation text completeness

**Question.** Which text carriers form the text of one annotation?

**Known.** `step.md` §8 "A presentation graph search types only the text
carrier it consumes" states the typing rule. The decoder returns the first
`TEXT_LITERAL` that a depth-first walk of the reachable graph reaches and
records nothing about the others
(`crates/cadmpeg-codec-step/src/reader/pmi.rs:822-853`).

**Need.** A feature control frame or a toleranced callout carries its symbol,
value, and datum letters as separate literals. The IR keeps one and drops the
rest, and which one survives depends on the producer's serialization order of
the callout contents. A multi-literal callout is indistinguishable from a
single-literal one in the output. We must know the composition rule for a
multi-compartment annotation.

### AP-05. Style precedence for independent styled items

**Question.** Which style applies when two `STYLED_ITEM` records target one
item and neither overrides the other?

**Known.** `step.md` §8 "An overriding style takes precedence for its
occurrence." settles the override case, and the decoder implements it. For
equal override depth the sort is stable over ascending instance-name order,
and the colour is assigned unconditionally, so the last styled item wins
(`crates/cadmpeg-codec-step/src/reader/presentation.rs:193-333`).

**Need.** A part with a design-view presentation and a second presentation for
another view carries two styled items on one face. The face colour follows
the higher instance name. The IR keeps both appearance bindings, so the
ambiguity survives there, but the scalar face colour that consumers read does
not, and no conflict is reported. We must know which presentation context
governs a colour.

### AP-06. Surface style side selection

**Question.** Which side of a surface does a `SURFACE_STYLE_USAGE` colour, and
which style applies to the visible side?

**Known.** The decoder returns the first colour it reaches in the style
aggregate (`crates/cadmpeg-codec-step/src/reader/presentation.rs:231-241`,
`:911-926`). The `side` enumeration of `SURFACE_STYLE_USAGE` is read nowhere
in the crate. A Part 21 aggregate of this kind is a set and has no order.

**Need.** An assignment that carries `.POSITIVE.` and `.NEGATIVE.` usages
resolves to whichever appears first in the serialized set, so the decoded
colour is the back-face colour whenever the producer writes it first. The
`StyleDomain` filter already blocks curve colours from reaching a face, so
the residual exposure is side selection and fill against rendering. We must
know the side rule before a set position may select a colour.

### AP-07. Triangle strip winding

**Question.** Which winding rule applies to the triangles of a
`TRIANGLE_STRIP`?

**Known.** `step.md` §8 "Triangle, strip, and fan indices address local
points." gives the index meaning and no winding rule. The decoder alternates
the first two indices on odd triangles
(`crates/cadmpeg-codec-step/src/reader/tessellation.rs:419-427`).

**Need.** The alternation matches the common strip convention, and the test
that covers it asserts the decoder's own output rather than a rule. If the
AP242 rule instead takes each consecutive triple in order, every odd triangle
of every strip faces inward. We must confirm the rule from the standard.

## 12. Envelope, lexis, and schema selection

### EL-01. Character encoding selection

**Resolved.** The major value in the raw `FILE_DESCRIPTION`
`implementation_level` selects the direct string repertoire. Values `4;1`,
`4;2`, and `4;3` use UTF-8. Earlier implementation levels use ISO-8859-1.
The reader applies this selection to every semantic string and retains
`\X2\` and `\X4\` escape decoding in both repertoires. Invalid direct UTF-8
bytes produce a metadata loss.

**Known.** `step.md` §2 and §6 define the repertoire and its header selector.

### EL-02. Exchange-structure detection

**Resolved.** Detection skips leading Part 21 whitespace and complete comments
and compares the `ISO-10303-21;` keyword sequence without ASCII case. A byte
order mark is not Part 21 whitespace and remains invalid.

**Known.** `step.md` §2 gives the outer grammar and applies whitespace and
comments at token boundaries. `step.md` §3 "ignore ASCII case. Canonical
spelling uses uppercase." makes keywords case-insensitive, and the lexer
implements that.

**Rule.** `detect`, `inspect`, and semantic decode apply the same leading
trivia and case-insensitive magic check as the parser. An incomplete leading
comment is not a recognized exchange; the parser reports it as malformed when
the input is forced to STEP.

### EL-03. Enumeration name characters

**Resolved.** An enumeration name begins with an ASCII letter and accepts
ASCII letters, digits, underscore, and hyphen until its closing dot.

**Known.** `step.md` §3 gives `enumeration = "." standard_name "."` and
`standard_name = letter (letter | digit | "_" | "-")*`.

**Rule.** The lexer applies the same `standard_name` character class to
enumerations and entity names. A name without its closing dot remains a lexical
error.

### EL-04. Signature section boundary

**Question.** Which `ENDSEC` terminates a SIGNATURE section?

**Known.** `step.md` §7 places the section end at the next `ENDSEC;`.

**Conflict.** The lexer locates `END-ISO-10303-21` first and then takes the
**last** `ENDSEC` before it (`crates/cadmpeg-codec-step/src/lex.rs:111-124`).
Both markers are matched as raw substrings, not as tokens. SG-01 and SG-03
record that the payload grammar is unknown, so nothing forbids a payload from
containing either literal. A payload containing `END-ISO-10303-21` makes the
file unparseable; a payload containing `ENDSEC` gives a section span that
holds bytes the specification places outside it. The fixture payload contains
neither, so no test distinguishes the two rules. The item needs a decision.

### EL-05. Schema identifier interpretation

**Question.** Which `FILE_SCHEMA` identifier governs decoding, and how does an
identifier select an application protocol and edition?

**Known.** `step.md` §6 "Schema identifiers select the application protocol
and edition." gives no identifier syntax and no edition mapping. The decoder
takes the first `FILE_SCHEMA` header record
(`crates/cadmpeg-codec-step/src/lib.rs:4438-4441`,
`crates/cadmpeg-codec-step/src/reader/mod.rs:701-711`), joins every listed
identifier into one string, and infers the AP242 edition by substring-testing
the object-identifier digits `442 4`, `442 3`, and `442 1`
(`lib.rs:4469-4477`). The same asserted edition-to-revision mapping writes the
identifiers of exported files (`lib.rs:136-166`). The header is not checked
for record order, arity, or duplicates, and `DataSection::parameters` is
parsed and never read.

**Need.** A revision outside the asserted three reports `edition
unspecified`; a file that lists an AP242 module beside an AP214 primary
identifier reports AP242. A second `FILE_SCHEMA` record, a missing one, or a
per-section `DATA('...')` declaration is dropped with no diagnostic, and a
schema outside AP203, AP214, and AP242 decodes by entity name with no
unrecognized-schema loss. We must know the identifier grammar and the
governing-schema rule.

### EL-06. Omitted entity name repair and anchor order

**Resolved.** Omitted inherited `name` repair runs after edition-3 anchor
values resolve.

**Known.** The parser holds a table of about 100 entity names and inserts an
empty name when a single-partial record of one of those names has a first
parameter that is neither a string nor omitted
(`crates/cadmpeg-codec-step/src/parse.rs:323-435`, `:671-675`). The repair
has its own diagnostic, produces a `NoncanonicalSourceSyntax` loss with byte
provenance, and is rejected by strict decode. `step.md` states no attribute
layout for these entities and this ledger has no item for the repair.

**Rule.** The parser reads each raw record, resolves all anchors in anchor
values and record parameters, then tests the first parameter of each single
named carrier against the carrier's inherited `name` slot. A resource anchor
that resolves to a string is therefore a real name and does not trigger repair.
The existing carrier table remains the scope of repair; other entity layouts
are not shifted.
