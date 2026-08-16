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

**Question.** What concrete `SaveDocFile` payload framing does each application-specific side-entry
writer use when its runtime property type is not registered by this codec?

**Known.** `Writer::addFile` stores one persistence writer and one unique member name for each
request, and the ZIP writer invokes that object's `SaveDocFile` for the complete member. The
generic archive boundary has no application-wide payload header. The producer source contains
concrete core and module side-entry writers, but the codec does not yet enumerate every remaining
runtime family that can reach this path.

**Need.** Enumerate the remaining unregistered runtime families and document the payload boundary
and framing emitted by each concrete writer.

**Note.** The full producer writer path settles the generic request and member boundary, not the
internal grammar of every family. This item remains narrowed; opaque retention is not a semantic
answer.

### AR-02. Application-specific side-entry values

**Question.** What does each field in an application-specific side-entry payload mean after its
concrete persistence writer has been identified?

**Known.** The exact persistence object writes the complete payload selected by `SaveDocFile`; core
serializers define typed payloads for several file-backed properties, including raw file bytes,
vector and placement lists, float/color/material lists, string tables, and element maps. The native
record retains the owning object, property, declared application type, XML bytes, side-entry bytes,
byte spans, lengths, and digests.

**Need.** Read the remaining concrete serializers and establish their field semantics before
transferring an unregistered side entry to a typed native or neutral record.

**Note.** The producer source now supplies a concrete writer lineage for this subset. Remaining
unregistered family fields still need their own writer evidence; native retention is not meaning
evidence.

## 2. GUI properties

### GP-01. Other GUI property grammars

**Question.** What value grammar remains for each GUI property runtime type not yet covered by the
specification?

**Known.** `ViewProvider` persistence reaches the same `TransactionalObject`,
`ExtensionContainer`, and `PropertyContainer` serializer used by application objects. The base
registry and an authored GUI witness establish these forms: Font/String, StringList/String,
IntegerList/I, Map/Item, Matrix `a11`-`a44`, Position/Direction `PropertyVector`, Quantity/Float,
and Rotation/PropertyRotation. The authored Sketcher witness also establishes a custom
`VisualLayerList` root with ordered `VisualLayer` records.

**Need.** Establish the complete remaining module-owned and dynamic GUI runtime registry, including
custom serializers and side-entry use, and validate those values without dropping the native
record.

**Note.** The authored headless GUI witness establishes the settled subset. The complete
module-owned and dynamic registry is settled by further authored witnesses and by the FreeCAD
property-editor registration source.

### GP-02. Other GUI property semantics

**Question.** What presentation semantics remain for each GUI property runtime type after the
settled core and Sketcher visual-layer subset?

**Known.** GUI properties use the application property's semantic type; GUI persistence does not
introduce a second interpretation. The authored Sketcher witness and its source class establish
that ordered `VisualLayer` records represent per-layer visibility, line pattern, and line width.
GUI records retain view-provider identity and each remaining undefined property's runtime type and
ordered values.

**Need.** Read the defining FreeCAD source and authored witness uses for each remaining
module-owned or dynamic runtime type before transferring it to a neutral presentation field.

**Note.** Core value semantics and the Sketcher visual-layer subset are now source-backed. The
remaining provider-specific presentation mapping is open; native retention is not semantic
evidence.

## 3. Persistent topology identity

### PT-03. Element-map carrier and owner selection

**Question.** Which `Part`, `ElementMap2`, and property carrier belong to one persistent element
map when a shape XML contains more than one candidate?

**Known.** Element maps are associated with a shape property and retain their source XML and map
order. The decoder rejects more than one `Part` or `ElementMap2` carrier in one exact-shape
property and rejects more than one enclosing property for a string table.

**Need.** Establish the exact producer cardinality and property association for duplicate
carriers. Duplicate candidates must be rejected or linked by a producer-defined discriminator.

**Conflict.** Commit `02c7628b3` removed this item and wrote a one-carrier rule into
`freecad_fcstd.md` without changing the decoder and without tracing the FreeCAD writer path for the
element-map carriers. Conservative
decoder rejection prevents a source-order choice but does not establish the legal cardinality or
shared ownership rule.

**Note.** Reopened by this QA pass.

### PT-04. Source topology index provenance

**Question.** What OCCT identity and traversal rules determine whether repeated placed roots or
equal shape-plus-location occurrences receive one shared or multiple indexed-map positions?

**Known.** FreeCAD assigns non-root topology positions through `TopExp::MapShapes` into a
`TopTools::IndexedMapOfShape`; root-shape positions use `TopoDS_Iterator` order. Part's
`TopoShapeExpansion` walks those one-based positions and binds element-map names to them. The
element-map root is the final map node after child maps. Differential authored witnesses show that
one OCCT shape identity at one placement, including a reversed use, occupies one indexed position;
a copied shape at that placement and a shape at a different placement occupy two positions.

**Conflict.** The child traversal order of `TopExp::MapShapes` for unequal nested topology is not
yet established from the OCCT map implementation or from authored differential witnesses.
topology_transfer.rs:1554-1598 uses a
decoder-owned walk, so its order cannot be assumed to match the producer for those cases.

**Need.** Read the OCCT map implementation or author differential witnesses for remaining unequal
nested topology. Nested compound evidence settles direct-child order and recursion; non-compound
unequal traversal and any unproven equality cases remain.

**Note.** Three authored nested-compound permutations establish persisted child order and
depth-first recursion, and match the topology-transfer walk. The item remains narrowed because
simple nested solids do not establish every OCCT topology class or equality case.

## 4. Exact-topology transfer

### XT-01. Edge endpoint child selection

**Question.** What child-use cardinality and orientation grammar does a producer-valid degenerate
edge use, and which malformed endpoint forms are invalid?

**Known.** Exact-shape records retain ordered and oriented topology children. A normal edge has
two endpoint uses, with `Forward` supplying the start and `Reversed` the end. A closed edge can
use the same vertex identity in those two orientations. In authored `NormalEdge.Shape.brp`, the
endpoint line is `+3 0 -2 0 *` at offset `0x2f8`; in `ClosedEdge.Shape.brp`, it is `+2 0 -2 0 *`
at offset `0x329`.

**Conflict.** A valid degenerate edge witness is not yet authored. The decoder's
topology_transfer.rs:1691-1721 rejection of duplicate orientations or missing endpoint uses is
not evidence for the producer's malformed-input boundary.

**Need.** Author a valid degenerate edge with the headless producer, or read the FreeCAD/OCCT
writer path that defines its endpoint uses. Author malformed duplicate, missing, and extra endpoint
witnesses before assigning their validity.

**Note.** This pass settled the normal and closed two-use forms from producer-authored bytes. The
degenerate and malformed forms remain open.

### XT-02. Edge representation selection and uniqueness

**Question.** When an edge has repeated 3D or polygon carriers, or more than one matching pcurve
representation for one face use, which representation supplies its neutral carrier or face pcurve?

**Known.** Exact-shape records retain all geometry carriers, locations, parameter ranges, and
pcurves. Polygon transfer is a fallback when an exact 3D curve is absent. A primary and secondary
pcurve can be one paired closed-surface representation rather than two matching representations
for one face use. Authored cylinder, sphere, and torus witnesses serialize `Curve2ds` counts of
6, 4, and 4 with both polygon tables at count zero.

**Conflict.** topology_transfer.rs:1723-1745 rejects multiple matching representations, but the
witnesses do not distinguish repeated 3D or polygon carriers from multiple matching pcurves for
one face use. The FreeCAD/OCCT writer path for repeated carriers and for duplicate matching pcurves
has not been traced, so their uniqueness, equivalence, and precedence remain open.

**Need.** Establish cardinality and precedence for repeated 3D carriers, repeated polygon carriers,
and multiple matching pcurves for one face use. Select by a serialized role or prove geometric
equivalence when duplicates are legal; otherwise define the exact malformed form.

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
source order, and design.rs:450-456 removes edges whose targets are not earlier. The FreeCAD
recompute and dependency-ordering source has not been traced, so source order and edge discard are
not established as the correct neutral result.

**Need.** Define a cycle projection that is stable and preserves the maximal admissible subset, or
refuse with an explicit loss. Do not source-order a cycle and silently discard its edges.

**Note.** This pass settled that the producer persists directed dependency cycles. Native retention
and the blocking `feature.cyclic-history` loss are safety policies, but they do not establish the
neutral relation. The prior specification text that prescribed source-order assignment and edge
discard was removed because it was decoder policy rather than producer evidence; the projection
remains open.

## 6. Product structure

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

## 9. Persistent graph admission
