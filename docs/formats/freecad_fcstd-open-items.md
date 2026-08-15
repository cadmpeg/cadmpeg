# FreeCAD .FCStd: Open Items

This document records unresolved FreeCAD .FCStd format questions. The specification records
settled byte semantics and invariants.

Each item has an identifier and these fields:

- Question
- Known
- Need
- Conflict
- Note

## 1. Application-specific side entries

### AR-01. Application-specific side-entry framing

**Question.** What byte framing does each application-specific side-entry family use when no
typed property grammar identifies the family?

**Known.** A side entry gets semantic meaning from a typed reference in Document.xml or
GuiDocument.xml. An unreferenced entry remains a named archive record. Application data without
a neutral representation retains its owning object and property.

**Need.** Establish the framing and record boundaries for each unregistered side-entry family.

**Note.** The closure records opaque retention as policy but provides no producer grammar or
independent witness for the unregistered families. Reopened after the side-entry closure.

### AR-02. Application-specific side-entry values

**Question.** What does each field in an application-specific side-entry family mean when no
typed property grammar identifies the family?

**Known.** The native record retains the owning object, property, declared application type,
links, source order, XML bytes, side-entry bytes, byte spans, lengths, and digests.

**Need.** Establish field semantics before transferring an unregistered side entry to a typed
native or neutral record.

**Note.** Native retention prevents unsafe interpretation but does not establish field semantics
or prove that an unregistered type has no neutral meaning. Reopened after the side-entry closure.

## 2. GUI properties

### GP-01. Other GUI property grammars

**Question.** What value grammar does each GUI property runtime type use when the specification
does not define that type?

**Known.** Undefined GUI properties retain their owner, runtime type, status, ordered value
elements, side-entry references, exact XML, and byte range.

**Need.** Establish each remaining runtime type grammar and validate its values without dropping
the native record.

**Note.** Exact handling for selected material and color-list types does not establish the
grammar of the remaining GUI types. The closure still has no complete producer registry or
independent witness for the unregistered set.

### GP-02. Other GUI property semantics

**Question.** What presentation value does each GUI property runtime type represent when the
specification does not define that type?

**Known.** GUI records retain view-provider identity and each undefined property's runtime type
and ordered values.

**Need.** Establish the value semantics before transferring an unregistered GUI property to a
neutral presentation field.

**Note.** Native retention is not semantic evidence. An unregistered property can still have
neutral meaning; the closure does not establish that every such type is opaque.

## 3. Persistent topology identity

### PT-04. Source topology index provenance

**Question.** What OCCT identity and traversal rules determine whether repeated placed roots or
equal shape-plus-location occurrences receive one shared or multiple indexed-map positions?

**Known.** FreeCAD assigns non-root topology positions through `TopExp::MapShapes` into a
`TopTools::IndexedMapOfShape`; root-shape positions use `TopoDS_Iterator` order. Part's
`TopoShapeExpansion` walks those one-based positions and binds element-map names to them. The
element-map root is the final map node after child maps. Differential authored witnesses show that
one OCCT shape identity at one placement, including a reversed use, occupies one indexed position;
a copied shape at that placement and a shape at a different placement occupy two positions.

**Conflict.** The producer call sites and witnesses do not establish the child traversal order of
`TopExp::MapShapes` for unequal nested topology. topology_transfer.rs:1554-1598 uses a
decoder-owned walk, so its order cannot be assumed to match the producer for those cases.

**Need.** Read the OCCT map implementation or author differential witnesses that distinguish
unequal nested topology traversal. Then compare those positions with topology transfer and
preserve the producer's order.

**Note.** This pass settled the FreeCAD producer's map owner, root iterator, one-based positions,
element-map binding, and identity behavior for repeated, copied, relocated, and reversed witness
uses. Nested traversal order remains open.

## 4. Exact-topology transfer

### XT-01. Edge endpoint child selection

**Question.** What child-use cardinality and orientation grammar does a producer-valid degenerate
edge use, and which malformed endpoint forms are invalid?

**Known.** Exact-shape records retain ordered and oriented topology children. A normal edge has
two endpoint uses, with `Forward` supplying the start and `Reversed` the end. A closed edge can
use the same vertex identity in those two orientations. In authored `NormalEdge.Shape.brp`, the
endpoint line is `+3 0 -2 0 *` at offset `0x2f8`; in `ClosedEdge.Shape.brp`, it is `+2 0 -2 0 *`
at offset `0x329`.

**Conflict.** No valid producer-authored degenerate edge is available. The decoder's
topology_transfer.rs:1691-1721 rejection of duplicate orientations or missing endpoint uses is
not evidence for the producer's malformed-input boundary.

**Need.** Author a valid degenerate edge or read the producer/kernel writer path that defines its
endpoint uses. Obtain independent evidence for malformed duplicate, missing, and extra endpoint
forms before assigning their validity.

**Note.** This pass settled the normal and closed two-use forms from producer-authored bytes. The
degenerate and malformed forms remain open.

### XT-02. Edge representation selection and uniqueness

**Question.** When an edge has multiple 3D curve, polygon, or matching curve-on-surface
representations, which representation supplies its neutral carrier and face pcurve?

**Known.** Exact-shape records retain all geometry carriers, locations, parameter ranges, and
pcurves. Polygon transfer is a fallback when an exact 3D curve is absent.

**Conflict.** topology_transfer.rs:1723-1745 rejects multiple matching representations. No
producer or kernel rule establishes representation uniqueness, equivalence, or precedence for
legal repeated carriers.

**Need.** Establish representation cardinality and precedence. Select by a serialized role or
prove geometric equivalence when duplicates are legal; otherwise define the exact malformed form.

**Note.** This pass settled the valid paired closed-surface pcurve form and its separation from
polygon carriers. Repeated 3D or polygon carriers and multiple matching pcurves for one face use
remain open.

## 5. Design projection

### DP-02. Sketch profile seed order

**Question.** Which neutral seed rule applies when the producer does not persist a profile-chain
seed?

**Known.** FreeCAD writes separate ordered `GeometryList` and `ConstraintList` values. The authored
`disconnected_a.FCStd` and `disconnected_b.FCStd` witnesses contain the same two disconnected
chains with their geometry-list orders exchanged; neither `Document.xml` contains a profile-chain
or seed record.

**Conflict.** design.rs:2385-2410 must select a neutral seed from decoder-owned data because the
producer does not persist one. Geometry-list order establishes source order but does not select a
neutral seed rule by itself.

**Need.** Settle the neutral seed rule and retain the persisted entity ordinal in the decision.

**Note.** This pass settled that the producer persists geometry and constraint order but no
profile-chain seed. The neutral seed decision remains open.

### DP-03. Sketch profile junction ambiguity and tolerance

**Question.** What neutral endpoint-equivalence and junction policy applies when the producer
persists coordinates with optional constraint operands but no junction tolerance or tie-break?

**Known.** FreeCAD writes endpoint coordinates in `GeometryList` and ordered constraint operands in
`ConstraintList`; it writes no generic endpoint-junction tolerance or junction-selection field.
The authored `junction_coordinates_only.FCStd` witness has three lines meeting at one coordinate
with `ConstraintList count="0"`. `junction_two_constraints.FCStd` has the same geometry with two
coincident constraints naming two continuations.

**Conflict.** design.rs:2474-2499 supplements constraints with coordinate matching.
endpoints_match_by_roundoff at design.rs:2593-2603 uses 64 machine epsilons scaled by coordinate
magnitude. The producer witnesses establish coordinates and optional constraints, not this
neutral numeric boundary or the full admissible profile topology.

**Need.** Settle endpoint equivalence and admissible profile topology for unconstrained and
multi-constraint junctions. An ambiguous junction must use constraint identity, an explicit
source-order rule, or an attributable refusal.

**Note.** This pass settled the producer-side absence of a generic junction tolerance and
tie-break. The neutral junction policy remains open.

### DP-05. Dependency-cycle ordinal fallback

**Question.** What neutral projection applies when feature dependencies, parents, or expressions
form a cycle?

**Known.** The native graph retains cycles. FreeCAD can persist `First -> Second` and
`Second -> First` as two `ObjectDeps` records and matching `PropertyLink` values. The neutral graph
must use a stable maximal subset whose targets precede their consumers, or carry an explicit
blocking loss.

**Conflict.** design.rs:679-688 marks all remaining objects cycle-affected, assigns ordinals by
source order, and design.rs:450-456 removes edges whose targets are not earlier. The specification
now records this policy, but no producer cycle projection establishes that source order and edge
discard are the correct neutral result.

**Need.** Define a cycle projection that is stable and preserves the maximal admissible subset, or
refuse with an explicit loss. Do not source-order a cycle and silently discard its edges.

**Note.** This pass settled that the producer persists directed dependency cycles. Native retention
and the blocking feature.cyclic-history loss are a safety improvement, but they do not establish
the neutral relation. The projection remains open.

### DP-07. Legacy point carrier provenance

**Question.** Does any FreeCAD producer version write a declared `Part::GeomPoint` with a `Point`
carrier instead of the current `GeomPoint` carrier?

**Known.** `PropertyGeometryList::Save` writes the `Geometry` type attribute from the geometry
runtime type. `GeomPoint::Save` writes `GeomPoint`; the other registered geometry writers use the
carrier tags recorded in the specification.

**Need.** Establish a producer source path or independent witness for the historical `Point` tag,
including the producer version and its value grammar.

**Conflict.** The decoder accepts `Point` as a compatibility carrier for `Part::GeomPoint`, but no
producer evidence for that alias is recorded.

**Note.** This pass settled the current producer runtime-name/carrier-tag mapping and rejects a
registered name paired with another known carrier. The historical point alias remains open.

## 6. Product structure

### PR-03. Product named carrier neutral projection

**Question.** Should a valid `VisibilityList` bit string populate neutral
`Occurrence.visible`, or remain only in the retained typed property until the
frozen product goldens can change?

**Known.** The current producer declares every named product carrier with the
runtime types, roots, and cardinalities written in the specification. The
decoder validates those carriers and retains the typed property XML and bit
string in the native application record.

**Need.** Settle the neutral visibility projection without regenerating the
frozen product goldens.

**Conflict.** The producer writes a valid `BoolList` bit string, but projecting
it into `Occurrence.visible` changes the existing product golden output.

**Note.** This pass settled the original runtime-type and value-cardinality
subset with producer source and an authored witness. The neutral visibility
projection remains a smaller implementation question.

## 7. Assembly joints

## 8. Attachment and assembly

### AT-02. Native map-mode representation

**Question.** Should the neutral `AttachmentRecord.map_mode` expose the fixed `MapMode` name or
retain the persisted zero-based index after the producer enum has been validated?

**Known.** FreeCAD writes `AttachmentSupport` as `App::PropertyLinkSubList` and `MapMode` as one
`App::PropertyEnumeration` `Integer` value. The specification records the complete index-to-name
table and the out-of-range rule. The decoder now enforces those carriers and rejects invalid
indexes while retaining the index text in the existing native field.

**Need.** Settle the neutral field spelling without changing the frozen golden snapshots in this
migration.

**Conflict.** The producer name is semantic, but the existing native arena contract stores the
persisted index text.

**Note.** This pass settled the producer carrier grammar, support cardinality, fixed enum table,
and out-of-range behavior with current source and an authored witness. The neutral spelling is a
smaller remaining implementation question.
