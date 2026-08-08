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

**Known.** `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." through `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." state that a REFERENCE entry binds an external occurrence name to a resource URI and that a target outside the exchange structure is an external dependency.

**Need.** We must know the rules to identify the external resource that a relative URI selects.

### ER-02. Resource access

**Question.** Which retrieval and authentication procedure applies to each external resource URI?

**Known.** `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." identifies URI targets outside the exchange structure as external dependencies. The clear-text exchange structure does not contain an access procedure.

**Need.** We must know the procedure to obtain the selected external resource.

### ER-03. Resource composition

**Question.** How does each external resource combine with the local instance graph?

**Known.** `step.md` §5 "Entity instance names share one namespace across all DATA sections." through `step.md` §5 "Entity instance names share one namespace across all DATA sections." define identity and reference resolution inside the DATA sections. `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." through `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." define external occurrence bindings and external dependencies.

**Need.** We must know the composition rule to resolve cross-resource identities and build one product graph.

### ER-04. Resource cache identity

**Question.** Which URI components and resource metadata determine whether two external resource references identify the same cached resource?

**Known.** `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." and `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." state that each REFERENCE entry contains a resource URI. The specification gives no cache-identity rule.

**Need.** We must know the identity rule to reuse a retrieved resource without combining different resources.

## 2. AP242 BO-Model sidecars

### BM-01. Sidecar envelope

**Question.** What XML grammar and file relationship identify an AP242 BO-Model sidecar?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" identify an AP242 BO-Model XML sidecar as an encoding that is distinct from the Part 21 clear-text exchange structure.

**Need.** We must know the envelope to detect, parse, and associate the sidecar with its Part 21 exchange structure.

### BM-02. Sidecar composition

**Question.** How do AP242 BO-Model XML identities and values combine with the Part 21 instance graph?

**Known.** `step.md` §5 "Entity instance names share one namespace across all DATA sections." through `step.md` §5 "Entity instance names share one namespace across all DATA sections." define identity and reference resolution inside the Part 21 DATA sections. The specification gives no cross-encoding composition rule.

**Need.** We must know the composition rule to build one product graph from the Part 21 exchange structure and its sidecar.

## 3. Containers and other encodings

### CE-01. ZIP container layout

**Question.** Which ZIP entries, names, metadata, and relationships form an edition-3 exchange container?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" identify a ZIP container as distinct from a clear-text Part 21 exchange structure. `step.md` §2 "A clear-text exchange structure uses this outer grammar:" defines the clear-text outer grammar.

**Need.** We must know the layout to locate and identify each exchange resource in the container.

### CE-02. ZIP resource composition

**Question.** How do references between exchange resources in an edition-3 ZIP container resolve?

**Known.** `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." through `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." define resource names and URIs in a Part 21 REFERENCE section. The specification gives no container-relative resolution rule.

**Need.** We must know the resolution rule to combine the contained resources into one product graph.

### CE-03. Part 28 XML grammar

**Question.** What XML grammar represents an AP203, AP214, or AP242 exchange structure in Part 28?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" define Part 21 clear text and identify Part 28 XML as a distinct encoding.

**Need.** We must know the grammar to parse record boundaries, values, and references from Part 28 XML.

### CE-04. Part 28 graph mapping

**Question.** How does each Part 28 XML construct map to the entity graph and invariants in `step.md`?

**Known.** `step.md` §5 "A parameter is an entity reference, value reference, named entity constant," through `step.md` §5 "Entity instance names share one namespace across all DATA sections." define Part 21 values, records, identities, and references. The specification gives no Part 28 mapping.

**Need.** We must know the mapping to apply schema decoding to a Part 28 exchange structure.

### CE-05. Part 26 binary grammar

**Question.** What HDF5 layout represents an AP203, AP214, or AP242 exchange structure in Part 26?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" define Part 21 clear text and identify Part 26 binary or HDF5 as a distinct encoding.

**Need.** We must know the layout to parse record boundaries, values, and references from Part 26 data.

### CE-06. Part 26 graph mapping

**Question.** How does each Part 26 HDF5 construct map to the entity graph and invariants in `step.md`?

**Known.** `step.md` §5 "A parameter is an entity reference, value reference, named entity constant," through `step.md` §5 "Entity instance names share one namespace across all DATA sections." define Part 21 values, records, identities, and references. The specification gives no Part 26 mapping.

**Need.** We must know the mapping to apply schema decoding to a Part 26 exchange structure.

## 4. User-defined names

### UD-01. User-defined entity semantics

**Question.** What entity semantics does each user-defined `!` entity name select?

**Known.** `step.md` §3 "user_name" defines the syntax of a user-defined name. `step.md` §5 "Entity instance names share one namespace across all DATA sections." through `step.md` §5 "Entity instance names share one namespace across all DATA sections." require an unknown entity to retain its name, complete spans, and links to other named opaque records.

**Need.** We must know the semantics to transfer a user-defined entity to typed native or neutral records.

### UD-02. User-defined type semantics

**Question.** What value semantics does each user-defined `!` type name select?

**Known.** `step.md` §3 "user_name" defines the syntax of a user-defined name. `step.md` §5 "A parameter is an entity reference, value reference, named entity constant," through `step.md` §5 "A parameter is an entity reference, value reference, named entity constant," define a typed parameter as a name with one parameter.

**Need.** We must know the semantics to decode the wrapped parameter as a typed value.

## 5. Signatures

### SG-01. Signature method selection

**Question.** Which SIGNATURE field identifies the signature method and its parameters?

**Known.** `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." through `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." define the complete byte boundary of a SIGNATURE section. The specification gives no field grammar for its content.

**Need.** We must know the selection rule to choose the correct signature verification method.

### SG-02. Signed byte sequence

**Question.** Which exact bytes does each signature method authenticate?

**Known.** `step.md` §2 "A clear-text exchange structure uses this outer grammar:" through `step.md` §2 "A clear-text exchange structure uses this outer grammar:" place each SIGNATURE section after the exchange terminator. `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." through `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." define the byte boundary of the SIGNATURE section.

**Need.** We must know the byte sequence to calculate the verification input.

### SG-03. Signature value encoding

**Question.** How does each signature method encode its signature value and verification material in the SIGNATURE section?

**Known.** `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." through `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." require retention of the complete SIGNATURE byte range. The specification gives no field grammar for the retained content.

**Need.** We must know the encoding to extract the signature value, keys, certificates, and method parameters.

### SG-04. Signature verification result

**Question.** Which validation conditions make each signature valid, invalid, or indeterminate?

**Known.** `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." through `step.md` §7 "REFERENCE entries bind an external entity or value occurrence name to a resource URI." define structural retention only. The specification gives no cryptographic validation conditions.

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

**Resolved.** A `PCURVE` definition must resolve to exactly one item in its
`DEFINITIONAL_REPRESENTATION`. The reader decodes 2D line, analytic conic,
polyline, NURBS, trimmed, offset, and affine-replica carriers. A 2D line
uses its referenced point and vector directly; its coordinates are then
converted once into the owning surface chart. A recursive carrier returns no
typed geometry when an active record repeats or the graph reaches depth 256;
the active set is released on every return path. Unsupported composite or
otherwise unrecognized 2D carriers remain named opaque records and are not
attached to a coedge. Topology that needs such a carrier records a
machine-readable pcurve omission loss.

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

**Resolved.** Each representation's `GLOBAL_UNIT_ASSIGNED_CONTEXT` supplies
the length and plane-angle scales for that representation and its reachable
representation-item closure. A carrier shared by representations must have
one equal scale in every context. A conflicting carrier has no per-carrier
override, uses the document fallback scale, and produces a geometry loss.
Unscoped values use the document fallback scale. The resolved scales reach
geometry, PMI, tessellation, topology, and validation consumers.

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

**Resolved.** A pcurve has no independent angular unit. Its coordinates use
the parameterization of its owning surface after the representation and
record-specific unit scales have been applied. The reader does not generate
degree/radian alternatives or rescale an angular axis from endpoint evidence.
If the declared coordinates do not form a usable topological carrier, the
coedge remains without a pcurve and the machine-readable topology loss records
that omission.

### PC-02. Synthesized pcurve chart

**Question.** May the decoder replace a pcurve's parameterization with an
affine map that it derives from the edge endpoints?

**Resolved.** Endpoint-derived calibration is allowed only when it preserves
every source coordinate. A destination axis may have zero scale only when the
source coordinate is constant over the complete declared pcurve interval.
Distinct source and destination endpoint values still use an affine map. A
source axis with equal endpoints but interior variation, or a varying source
axis mapped to equal destination endpoints, rejects the synthesized variant;
the pcurve remains opaque rather than losing its locus. Coordinate bounds use
33 samples over the declared interval, including both endpoints.

### PC-03. Surface chart remap from the pcurve population

**Resolved.** A pcurve uses the parameterization of its owning surface. The
surface population does not define or modify that chart. A linear extrusion
inherits the directrix parameter as `u`; a revolution inherits the directrix
parameter as `v` and uses the surface angle as `u`. A non-linear directrix
therefore keeps its native parameterization. The reader does not infer a
surface-wide affine map from trimmed pcurves. A bounded procedural pcurve may
still receive a use-scoped endpoint calibration under PC-02.

### PC-04. Chart write-back to the shared pcurve

**Resolved.** The source pcurve carrier is immutable. A chart variant derived
from one coedge's endpoint fit is a use-scoped pcurve carrier. The coedge owns
the derived carrier through its `PcurveUse`; another coedge may select a
different variant without changing the source carrier or the first coedge's
parameter range.

**Rule.** If selection keeps the source geometry, the coedge references the
source pcurve. If selection changes the geometry, the reader creates a
canonical use-scoped pcurve identity and copies the source carrier metadata.
The source pcurve remains available for other uses and for opaque-record
ownership. When no typed use retains the source identity, normal carrier
retention removes the unowned neutral source carrier while preserving its raw
STEP record as opaque data.

### PC-05. Periodic trim interval

**Resolved.** A cyclic trim follows the directed parameter branch. For a
forward sense, if the second select is below the first, add one basis period
to the second select. For a reversed sense, if the first select is below the
second, add one basis period to the first select. The stored local domain is
the absolute directed span after that adjustment. Non-cyclic trims use the
stored selects without adjustment. The same rule applies independently to
the U and V axes of `RECTANGULAR_TRIMMED_SURFACE`.

### PC-06. Default placement reference direction

**Resolved.** An omitted or parallel `AXIS2_PLACEMENT_3D.ref_direction` uses
the projection of global +X onto the plane normal to the axis. When the axis
is within `1e-12` of parallel to X, it uses global +Y before projection. The
STEP reader applies this rule locally, so a neutral stability helper cannot
change STEP chart semantics.

### PC-07. Ellipse semi-axis canonicalization

**Resolved.** The IR keeps `major_radius ≥ minor_radius`. For
`semi_axis_1 < semi_axis_2`, it stores `cross(axis, ref_direction)` as the
major direction and maps the source parameter with `v = u − π/2`. Numeric
`TRIMMED_CURVE` selectors apply that phase after angular unit conversion;
Cartesian selectors invert the canonical geometry directly. Replicas, nested
trims, and spatial offsets inherit the phase.

## 9. Body and root identity

### BR-01. Topology root identity

**Resolved.** The topology-root cache key includes the governing root type,
the resolved shell identities, and shell orientations. Multiple
representations that reach one root of the same type reuse its committed body
identity. Distinct root records retain distinct bodies when their root types
differ, even when they share shell carriers. Body kind is therefore
independent of instance-number order.


### BR-02. Outer and void shell roles

**Resolved.** `BREP_WITH_VOIDS` attribute 1 is the outer shell and attribute 2
contains the void shells. The IR stores the outer role in the first
`Region.shells` entry. The reader rejects a solid root when the outer shell
splits into multiple connected components, because the extra component cannot
retain the outer role in the current IR. Sheet and general roots still retain
each valid connected component.

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

**Resolved.** `transform_item_1` belongs to `rep_1` and `transform_item_2`
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

### PS-03. Repeated mapped placements of one representation

**Resolved.** A body-producing representation may have several standalone
`MAPPED_ITEM` records only when all records resolve to one transform. Distinct
placements cannot be represented by one `Body.transform`; the reader leaves
that body unplaced and reports `AssemblyPlacementsNotTransferred`. Mappings
owned by product occurrences use occurrence transforms and are not part of
this rule.

### PS-04. Product and product-definition identity

**Resolved.** A CADIR product definition represents one STEP
`PRODUCT_DEFINITION` view. A product with one definition keeps the historical
`step:product:product#<product>` identity. When one `PRODUCT` has multiple
definitions, each view receives a distinct deterministic identity suffixed by
its definition instance. Shape bodies and definition descriptions bind to
their own view; they are not merged. Each definition not named as a usage
receives one root occurrence, and every usage occurrence references the
specific child definition view.

### PS-05. Mapped-item scope for occurrence placement

**Question.** Must a `MAPPED_ITEM` that supplies an occurrence placement
belong to the parent's own representation?

**Resolved.** An inferred occurrence placement must be a mapped item directly
listed by a representation of the occurrence's parent definition. The reader
ignores mapped items listed by unrelated representations. If no scoped mapping
remains, the occurrence keeps identity placement and reports
`AssemblyPlacementsNotTransferred`. Occurrence-owned shape representations and
the complete parent-representation sequence inference are evaluated before
this fallback.

### PS-06. Validation representation item count

**Question.** How many measure items may one geometric-validation
representation carry?

**Resolved.** A validation representation transfers every referenced item.
Area, volume, and centroid items are evaluated independently. An unsupported
item reports a warning naming that item and does not suppress other items in
the same representation. Repeated item references are evaluated once.

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

**Resolved.** The characteristic representation collects all reachable
measure representation items from its item aggregate. A unique item named
`nominal value` supplies the nominal. Without that name, exactly one measure
item supplies it. Multiple unnamed items remain ambiguous, produce a metadata
warning, and do not select a source-order value.

### AP-03. Geometric tolerance kind selection

**Resolved.** A complex geometric tolerance takes its kind from the exact
geometric-tolerance leaf partial. Inherited base and modifier partials do not
select the kind. The reader uses the same exact leaf table for direct and
complex instances, so a non-leaf name that ends in `_TOLERANCE` remains an
opaque source record instead of changing the leaf kind. The writer emits each
supported leaf entity by its corresponding IR kind.

### AP-04. Annotation text completeness

**Resolved.** A direct text carrier or a graph with exactly one reachable text
carrier supplies the presentation text. A graph with multiple reachable text
carriers has no ordered composition in this model, so the text remains absent,
a metadata loss is emitted, and every carrier remains a named opaque record
with its source links. The reader never selects a carrier by traversal or
serialization order.

### AP-05. Style precedence for independent styled items

**Resolved.** Override chains remove their overridden base styles. Independent
effective styles all remain appearance bindings. The reader sets a neutral
face or body scalar color only when those styles produce one distinct color;
duplicate colors collapse to that value. Conflicting colors leave the scalar
unset and emit a `MetadataNotTransferred` loss naming every contributing
styled item. Source instance order never selects a scalar color.

### AP-06. Surface style side selection

**Resolved.** `SURFACE_STYLE_USAGE` applies its style to the side named by its
`side` enumeration: `.POSITIVE.` is the surface-normal side, `.NEGATIVE.` is
the opposite side, and `.BOTH.` applies to both sides. CADIR stores one neutral
surface color, so the reader selects `.BOTH.` before `.POSITIVE.` before
`.NEGATIVE.` independently of aggregate serialization order.

### AP-07. Triangle strip winding

**Resolved.** A strip with indices `v[0]` through `v[n]` produces
`[v[i], v[i+1], v[i+2]]` for an even `i` and
`[v[i+1], v[i], v[i+2]]` for an odd `i`. Fans keep their first index and
advance the other two. The reader applies this rule and the regression covers
the first two triangles of one strip.

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

**Resolved.** Signature sections follow `END-ISO-10303-21;`. Each section
starts with `SIGNATURE;` and ends at its own token `ENDSEC;`. The decoder
retains every complete section range in source order.

**Known.** `step.md` §2 and §7 define the post-terminator placement and the
base64 content. The lexer finds the first token-boundary `ENDSEC;` after each
`SIGNATURE;`; it does not search for the exchange terminator or merge adjacent
signature sections.

### EL-05. Schema identifier interpretation

**Resolved.** `FILE_SCHEMA` contains one or more unique string identifiers.
The first identifier governs the application protocol and edition. An
identifier is a schema name with an optional brace-delimited object identifier
whose components are space-separated signed decimal integers. The decoder selects
AP242 edition 1, 2, or 3 only for the exact long-form name and exact object
identifiers `1 0 10303 442 1 1 4`, `1 0 10303 442 3 1 4`, and
`1 0 10303 442 4 1 4`. Other AP242 object identifiers report an unspecified
edition. Later identifiers remain metadata and do not change the selection.

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
