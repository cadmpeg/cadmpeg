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

### AR-03. Typed geometry side-entry cardinality

**Question.** How many side entries can one `PropertyMeshKernel` or `PropertyPointKernel` property
reference, and which entry contains the geometry payload?

**Known.** The current specification defines one typed payload per property. Property records
retain every side-entry request in source order. The current transfer path rejects more than one
side entry and otherwise reads the first entry.

**Need.** Establish producer cardinality and entry selection for both runtime types. The decoder
must reject invalid cardinality or identify the payload entry from the typed value grammar.

**Conflict.** Commit `02c7628b3` removed this item and wrote the one-entry rule into
`freecad_fcstd.md` without changing the decoder and without tracing the FreeCAD writer path. Its
existing malformed-input test establishes only decoder policy, not producer cardinality.

**Note.** Reopened by this QA pass. `AR-05` records the separate value-root and side-entry
association gap.

### AR-05. Typed geometry value-root association

**Question.** How are multiple `Mesh` or `Points` value roots associated with the one side-entry
payload and its transform?

**Known.** The specification states one typed value root per property and zero or one file
reference. `application_geometry.rs:21-61` checks only the number of collected side entries,
selects the first matching archive entry, and does not validate the value-root count.
`application_geometry.rs:160-186` selects the first descendant `Points` root carrying `mtrx`.
`persistence.rs:438-492` retains every descendant value and every file attribute.

**Need.** Establish the producer root/file association for both runtime types. The decoder must
reject multiple roots or select the payload and transform from one unambiguous typed value.

**Conflict.** A property containing one `Points` root without a file and a later `Points` root with
a file has one collected side entry, so it passes the current cardinality check while the
transform is read from the first root and the payload from the second. The existing malformed
test covers two roots that each have a file and therefore does not exercise this mismatch.

**Note.** New item from this QA pass. The specification rule is not yet established from the
FreeCAD writer path or from authored multi-root witnesses.

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

### GP-09. Camera position child cardinality

**Question.** Can a persisted GUI camera contain more than one `Position` child, and if so which
position supplies the camera state?

**Known.** GUI admission requires schema version 1 and exactly one direct `Camera`. The camera
state retains descendant values, while `gui.rs:492-523` selects the first `Position` value and
does not check for duplicates. The specification settles optional finite, nonzero position and
orientation values but not `Position` child cardinality.

**Need.** Establish the producer cardinality and selection rule for camera `Position` children.
Reject an ambiguous camera or retain an explicitly identified source value before projecting
camera state.

**Conflict.** Two `Position` children with conflicting coordinates are accepted and the first is
projected. The second remains in the native value list without a refusal or loss, so source order
silently decides the neutral camera state.

**Note.** New item from this QA pass.

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

### DP-07. Legacy point carrier provenance

**Question.** Does any FreeCAD producer version write a declared `Part::GeomPoint` with a `Point`
carrier instead of the current `GeomPoint` carrier?

**Known.** `PropertyGeometryList::Save` writes the `Geometry` type attribute from the geometry
runtime type. `GeomPoint::Save` writes `GeomPoint`; the other registered geometry writers use the
carrier tags recorded in the specification.

**Need.** Establish the FreeCAD source path or a saved witness document for the historical `Point`
tag, including the producer version and its value grammar.

**Conflict.** The decoder accepts `Point` as a compatibility carrier for `Part::GeomPoint`. The
FreeCAD writer history for that alias has not been traced.

**Note.** This pass settled the current producer runtime-name/carrier-tag mapping and rejects a
registered name paired with another known carrier. The historical point alias remains open.

### DP-09. Spreadsheet carrier and value-container selection

**Question.** Which property and XML value container supply spreadsheet cells and row or column
dimensions when more than one candidate matches the spreadsheet selectors?

**Known.** The design registry identifies the spreadsheet runtime types. `design.rs:748-764`
selects the first property whose type contains `PropertySheet` or whose name is `cells`, then the
first `Cells` descendant. `design.rs:865-907` selects the first matching column-width or
row-height property and the first matching dimension container. The specification settles the
used-cell graph and duplicate-cell validation but not selector cardinality or precedence.

**Need.** Establish the producer property and value-container cardinality and identity for cells,
column widths, and row heights. Reject ambiguous candidates or select them through an exact
runtime and container grammar.

**Conflict.** A spreadsheet with two matching properties or two matching value containers is
accepted and projected from the first match. A vendor-qualified type or a second container can
therefore change the neutral spreadsheet without an explicit ambiguity result.

**Note.** New item from this QA pass.

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

## 9. Persistent graph admission

### PG-05. Extension identity and nested-property ownership

**Question.** What cardinality and identity rule applies to multiple `Extensions` containers or
same-name `Extension` records under one object, and how are nested property containers owned?

**Known.** `persistence.rs:283-314` validates each `Extensions Count` independently and assigns
an order starting at zero within each container. `persistence.rs:316-343` binds a nested property
container to the first extension with the same owner and name. Native validation computes
extension IDs but checks owner existence without checking extension-ID uniqueness.

**Need.** Establish producer cardinality, name uniqueness, and ordering for extension records, or
reject duplicate candidates. Bind nested properties by the owning XML record rather than a
non-unique name lookup.

**Conflict.** Two same-name extensions in separate containers can receive the same generated ID,
and nested properties for repeated names all bind to the first matching extension. The resulting
native graph can lose extension ownership while passing the current extension referential checks.

**Note.** New item from this QA pass.

### PG-06. Object property-container cardinality

**Question.** Can one `ObjectData` record contain multiple direct `Properties` containers, and how
are those containers associated and ordered?

**Known.** Root document properties enforce at most one container. For each object,
`persistence.rs:316-343` iterates every direct `Properties` child and parses all of them under the
same object owner. The specification does not settle object-level container cardinality.

**Need.** Establish the producer cardinality and ordering rule for direct object property
containers. Reject ambiguous containers or retain an association that distinguishes them.

**Conflict.** Multiple direct containers with distinct property names are merged into one owner
without a container identity or ambiguity finding. Multiple containers can therefore alter source
ordering and property grammar while remaining accepted.

**Note.** New item from this QA pass.
