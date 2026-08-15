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

**Note.** Opaque retention is a safe decoder policy. It does not prove that no FreeCAD side-entry
grammar exists. No producer source or independent saved witness settles the unregistered
families.

### AR-02. Application-specific side-entry values

**Question.** What does each field in an application-specific side-entry family mean when no
typed property grammar identifies the family?

**Known.** The native record retains the owning object, property, declared application type,
links, source order, XML bytes, side-entry bytes, byte spans, lengths, and digests.

**Need.** Establish field semantics before transferring an unregistered side entry to a typed
native or neutral record.

**Note.** Opaque retention prevents unsafe interpretation. It does not establish field
semantics or prove that an unregistered type has no neutral meaning.

### AR-04. Shared side-entry logical ownership

**Question.** How does the logical byte ledger represent one archive entry referenced by more
than one property or typed payload?

**Known.** EntryRecord.referenced_by retains multiple semantic references while the byte span
has one archive-entry owner.

**Need.** Establish whether typed side entries can be shared. If sharing is valid, keep one byte
span with a many-owner relation. If sharing is invalid for a typed family, reject the conflicting
claims.

**Note.** The representation fix did not establish the FreeCAD producer rule. Writer::addFile
creates a distinct archive file record on each call, but this does not cover all possible
references or shared logical payloads.

## 2. GUI properties

### GP-01. Other GUI property grammars

**Question.** What value grammar does each GUI property runtime type use when the specification
does not define that type?

**Known.** Undefined GUI properties retain their owner, runtime type, status, ordered value
elements, side-entry references, exact XML, and byte range.

**Need.** Establish each remaining runtime type grammar and validate its values without dropping
the native record.

**Note.** Exact handling for selected material and color-list types does not establish the
grammar of the remaining GUI types. FreeCAD permits application-owned property types; no
complete producer registry or witness was found for the unregistered set.

### GP-02. Other GUI property semantics

**Question.** What presentation value does each GUI property runtime type represent when the
specification does not define that type?

**Known.** GUI records retain view-provider identity and each undefined property's runtime type
and ordered values.

**Need.** Establish the value semantics before transferring an unregistered GUI property to a
neutral presentation field.

**Note.** Native retention is not semantic evidence. An unregistered property can still have
neutral meaning; no producer source or independent file establishes that every such type is
opaque.

### GP-06. GUI camera derived-value admission

**Question.** Which numeric and geometric invariants must the GUI camera parser enforce before it
creates derived position and orientation fields?

**Known.** GUI schema 1 has one direct Camera. A decoded camera position and orientation must be
finite and nonzero when present.

**Conflict.** gui.rs:380-389 and 489-504 parse floating-point attributes and vectors without
finite or nonzero checks. A schema-1 Camera with NaN coordinates or a zero orientation can
therefore produce a neutral camera state that violates the specification.

**Need.** Reject or retain invalid camera values before neutral transfer. Apply the position and
orientation checks independently.

### GP-07. GUI schema attribute dispatch

**Question.** Are schema-version aliases invalid, and must the schema-1 camera rule apply only
after canonical attribute validation?

**Known.** FreeCAD writes the canonical SchemaVersion attribute. GUI schema 1 requires exactly
one direct Camera.

**Conflict.** gui.rs:58-68 reads only SchemaVersion and does not reject schemaVersion. A root with
schemaVersion=1 and no canonical attribute leaves schema_version unset and bypasses the schema-1
camera cardinality check.

**Need.** Use the canonical attribute rule shared by the document envelope, and reject aliases
or conflicting attributes before applying schema-specific cardinality rules.

**Note.** A hostile GuiDocument.xml with only the lowercase alias and zero or multiple Camera
elements is accepted past this gate. The existing GUI cardinality closure does not cover
attribute dispatch.

## 3. Persistence graph

### PG-04. Document property-container cardinality

**Question.** Can Document.xml contain more than one root-level Properties container?

**Known.** FreeCAD Document::Save writes one document PropertyContainer. Property names are
unique within that container.

**Conflict.** persistence.rs:255-265 selects the first root-level Properties element with find.
Later root-level Properties elements are ignored rather than rejected or retained.

**Need.** Enforce one root-level document Properties container and preserve the full property
graph when the input violates that cardinality.

**Note.** A hostile document with two root-level Properties containers loses every property in
the second container while decode still succeeds. The source writer and reader establish the
normal one-container form; the PG-03 closure did not validate root-container cardinality.

## 4. Persistent topology identity

### PT-04. Source topology index provenance

**Question.** Does the decoder's topology traversal reproduce the producer's persistent
indexed-map position for every placed occurrence?

**Known.** Persistent element-map names bind to topology occurrences. The element-map root is
the final map node after child maps.

**Conflict.** topology_transfer.rs:1554-1598 assigns indices with a decoder-owned depth-first
walk and a key composed of shape and transform. It does not read a producer index or cite a
FreeCAD or OCCT enumeration rule. Equal shape and transform occurrences can collapse to one key.

**Need.** Establish the producer indexed-map enumeration rule and carry that index through
topology transfer. Preserve distinct persistent occurrences when their source positions differ.

**Note.** The PT-02 closure corrected the counter scope across multiple roots. It did not prove
that the replacement walk matches the producer. A repeated source occurrence can receive a
decoder-inferred identity instead of its persisted element-map position.

## 5. Exact-topology transfer

### XT-01. Edge endpoint child selection

**Question.** What child-use cardinality and orientation combinations define the start and end
vertices of normal, closed, degenerate, and malformed edge records?

**Known.** Exact-shape records retain ordered and oriented topology children. Neutral edges
require explicit start and end vertex identities.

**Conflict.** topology_transfer.rs:1691-1721 requires one Forward and one Reversed child and
rejects duplicate orientations. No FreeCAD or OCCT source or independent witness establishes
that duplicate orientation uses are invalid, or that this is the complete valid endpoint grammar.

**Need.** Establish the valid endpoint forms and their orientation semantics. Handle each valid
form explicitly and reject only a form that cannot establish both endpoint identities.

**Note.** The closure changed an unverified selection rule into a refusal rule. A refusal is not
evidence that the producer cannot emit a valid non-manifold, degenerate, or seam-edge form.

### XT-02. Edge representation selection and uniqueness

**Question.** When an edge has multiple 3D curve, polygon, or matching curve-on-surface
representations, which representation supplies its neutral carrier and face pcurve?

**Known.** Exact-shape records retain all geometry carriers, locations, parameter ranges, and
pcurves. Polygon transfer is a fallback when an exact 3D curve is absent.

**Conflict.** topology_transfer.rs:1723-1745 rejects multiple matching representations. No
producer source or independent witness establishes representation uniqueness, equivalence, or
precedence for legal repeated carriers.

**Need.** Establish representation cardinality and precedence. Select by a serialized role or
prove geometric equivalence when duplicates are legal; otherwise define the exact malformed
form.

**Note.** The closure promotes refusal to a format invariant without primary evidence for
multiple representation cases.

### XT-03. Non-manifold radial order

**Question.** What source order defines the radial cycle when more than two coedges use the same
edge?

**Known.** Native topology retains ordered child uses and orientations. A neutral coedge has
one radial_next relation.

**Conflict.** topology_transfer.rs:1678-1689 links only two coedges and leaves three or more
self-radial. No producer source or independent non-manifold witness establishes that the source
has no radial order.

**Need.** Establish whether FreeCAD or OCCT supplies a radial order for non-manifold uses. If it
does not, retain unordered incidence or mark radial order unresolved.

**Note.** The closure promoted a neutral fallback to settled source semantics without evidence
for the non-manifold case.

## 6. Design projection

### DP-01. Forward declared dependencies

**Question.** Can a declared ObjectDeps target appear later than its dependent object in source
order?

**Known.** Declared dependencies and earlier link-property operands form the feature dependency
graph. A declared dependency can target a later declaration. The source-order restriction
applies to link operands, not declared dependencies.

**Conflict.** design.rs:420-449 filters every dependency by the target feature ordinal being
earlier than the consumer. A declared forward dependency remains native but disappears from the
neutral feature graph.

**Need.** Preserve every resolved declared feature dependency. Apply the earlier-source rule only
to link-property operands.

**Note.** The current specification and FreeCAD Document.cpp dependency writer allow the
forward-declaration case. The implementation still drops it.

### DP-02. Sketch profile seed order

**Question.** Which non-construction entity starts each oriented sketch profile chain?

**Known.** Sketch entities retain persisted source order and native identity. Profile chains must
be deterministic and attributable.

**Conflict.** design.rs:2371-2457 seeds each profile from the first remaining entity in a
decoder-owned ordered set. FreeCAD PropertyGeometryList.cpp proves persisted geometry-list order
but does not prove this disconnected-chain seed rule.

**Need.** Establish the producer-defined seed rule for each disconnected chain and retain the
persisted entity ordinal in the decision.

**Note.** The closure fixed an earlier decimal-string ordering defect. It did not establish the
source rule for disconnected profiles, so the first unused entity remains an unverified
semantic choice.

### DP-03. Sketch profile junction ambiguity and tolerance

**Question.** What endpoint tolerance connects two sketch entities, and what happens when more
than one unused entity meets the current endpoint?

**Known.** Constraints and persisted geometry can produce coincident endpoints. A neutral
profile chain asserts one ordered continuation and orientation at every junction.

**Conflict.** design.rs:2460-2485 supplements constraints with coordinate matching.
endpoints_match_by_roundoff at 2579-2589 uses 64 times machine epsilon scaled by the coordinate
magnitude. No FreeCAD tolerance or admissible profile topology supports this numeric boundary.

**Need.** Establish endpoint equivalence and the admissible profile topology. An ambiguous
junction must use constraint identity, an explicit source-order rule, or an attributable
refusal.

**Note.** The closure added ambiguity handling and a scale formula. The boundary remains a
decoder policy without a producer or kernel witness.

### DP-04. Design runtime and sketch-carrier dispatch

**Question.** Which exact runtime type and child value select a design feature or sketch geometry
family?

**Known.** Native records retain exact runtime types and ordered XML children. Known FreeCAD
families have family-specific value fields.

**Conflict.** design.rs:5066-5078 and 5185-5190 use contains for several design families.
Application-owned types containing a built-in token can therefore enter a built-in neutral
family. The specification requires exact dispatch and excludes subclass or substring
misclassification.

**Need.** Establish the complete exact design runtime registry and carrier grammar. Unknown or
conflicting carriers must remain native or be rejected.

**Note.** The closure removed one family of first-carrier ambiguity but did not remove the
substring dispatch paths.

### DP-05. Dependency-cycle ordinal fallback

**Question.** What neutral projection applies when feature dependencies, parents, or expressions
form a cycle?

**Known.** The native graph retains cycles. The neutral graph must use a stable maximal subset whose
targets precede their consumers.

**Conflict.** design.rs:650-675 falls back to the minimum source-order un-emitted object when no
object is ready. design.rs:420-449 then removes dependencies whose target ordinal is not earlier.

**Need.** Define a cycle projection that is stable and preserves the maximal admissible subset, or
refuse with an explicit loss. Do not source-order a cycle and silently discard its edges.

**Note.** A two-object A-to-B and B-to-A cycle emits one object by source order and drops the
back-edge from the neutral graph. Reordering the declarations changes the neutral relation while
decode still succeeds.

## 7. Product structure

### PR-01. Product runtime registry and membership

**Question.** Which exact runtime types and cardinality rules govern product records, container
membership, and linked prototypes?

**Known.** The specification names the exact product registry and retains other runtime types as
native records.

**Conflict.** product.rs product_kind admits App::Part, App::Link, App::LinkElement, and the
listed group types, but omits Assembly::AssemblyLink. FreeCAD AssemblyLink is a subclass of
App::Part and is a standard product carrier. A valid AssemblyLink therefore does not enter the
product transfer path as its application-defined product type.

**Need.** Establish the complete exact product registry, including AssemblyLink, and define
membership and prototype cardinality from producer source.

**Note.** AssemblyLink.h and AssemblyLink.cpp identify the runtime type and its App::Part
inheritance. The current exact-name closure is incomplete for that standard producer type.

### PR-02. Product carrier runtime types

**Question.** Which runtime type must each named product carrier have before its value enters a
neutral occurrence?

**Known.** FreeCAD defines LinkCopyOnChange as App::PropertyEnumeration and defines the related
link-copy carriers by their declared property types.

**Conflict.** product.rs:77-88 and the copy_on_change projection select properties by name.
enumeration_value is then applied without checking the declared runtime type. A property with
the name LinkCopyOnChange and an Integer value can enter the copy-on-change policy.

**Need.** Enforce the exact runtime type for every product carrier before projecting its value.
Retain or reject a wrong-type carrier without interpreting its child value.

**Note.** A hostile property with a valid-looking integer but the wrong runtime type changes
occurrence semantics while remaining a successful decode.

## 8. Semantic annotations

### SA-01. Runtime-type to annotation-kind mapping

**Question.** Which exact application runtime types represent dimensions, geometric tolerances,
datums, balloons, leaders, symbols, and text annotations?

**Known.** Native annotation records retain the exact runtime type. The neutral arena requires
separate semantic kinds.

**Conflict.** annotation.rs:167-225 uses a positive exact registry and sends every other type to
native retention. The FreeCAD TechDraw source tree contains additional standard derived drawing
types, including DrawViewArch, DrawViewDraft, DrawViewSpreadsheet, and DrawViewCollection; the
current registry has no exhaustive producer evidence or explicit exclusion for related semantic
types.

**Need.** Establish the complete exact runtime registry and kind mapping. Unknown application
types must remain native until their semantic family is established.

**Note.** Positive fixture cases prove listed mappings only. Inheritance alone does not prove a
neutral kind, but the current positive list cannot be treated as complete without an exhaustive
producer registry or explicit exclusions.

### SA-02. Annotation scalar and position property selection

**Question.** Which property carries the semantic scalar and position for each annotation runtime
type?

**Known.** The native property graph retains every named value independently. A neutral semantic
annotation has one optional scalar and one optional position.

**Conflict.** annotation.rs:228-330 selects values by property name and fixed priority, without
checking the expected runtime type for the annotation family. A wrong-type numeric value can
populate the neutral scalar or position.

**Need.** Map scalar and position carriers by exact runtime type and reject contradictory or
wrong-type carriers. Do not use property-name priority as semantic dispatch.

**Note.** FreeCAD Annotation.cpp, DrawViewAnnotation.cpp, and DrawRichAnno.cpp provide expected
property definitions for their types. The current path does not enforce those definitions.

## 9. TechDraw projection

### DG-01. TechDraw runtime-type classification

**Question.** Which exact runtime types enter the TechDraw arena and which drawing kind does each
type represent?

**Known.** Drawing records retain exact runtime type and source order. Core drawing dispatch uses
exact runtime names.

**Conflict.** drawing.rs:23-25 admits every type beginning with TechDraw::, while classify at
147-172 omits standard derived types such as DrawBrokenView, DrawComplexSection, DrawParametricTemplate,
DrawViewMulti, DrawViewArch, and DrawViewDraft. These valid types are admitted as DrawingKind::Other
or receive an incomplete inherited classification.

**Need.** Establish the complete exact TechDraw runtime registry and inheritance/type mapping.
Unknown extension types must remain native without suppressing known standard types.

**Note.** TechDraw PROPERTY_SOURCE declarations establish the omitted derived types and their
base classes. The positive exact list is not exhaustive.

### DG-02. Drawing link, type, and parameter selection

**Question.** What cardinality and precedence rules select drawing templates, scalar/vector
carriers, link properties, and repeated parameters?

**Known.** Drawing records retain all links and source properties. DrawView defines exact base
property types.

**Conflict.** drawing.rs:175-193 and 246-287 select values by name and parse their XML value
without enforcing the expected runtime type. links selects the first same-name property.
Consequently a wrong-type numeric carrier can populate a drawing parameter and conflicting
same-name paths can disagree.

**Need.** Establish TechDraw property definitions, cardinalities, and precedence from FreeCAD
source. Gate every semantic carrier by its exact runtime type and reject contradictory duplicates.

**Note.** DrawView.cpp, DrawPage.cpp, and DrawViewPart.cpp provide the normal base property
definitions. The current generic name/value path does not enforce them.

### DG-03. Drawing numeric and geometric admission

**Question.** Which numeric and geometric invariants must drawing transfer enforce before it
creates neutral view fields?

**Known.** Drawing view position is finite, scale is positive, and projection direction is
finite and nonzero.

**Conflict.** drawing.rs:108-135 and 246-287 validate parseability but not finiteness, scale
sign, or nonzero direction. lib.rs:1238-1239 transfers drawing records without running final
validation. A negative Scale, NaN value, or zero Direction can therefore enter the neutral graph.

**Need.** Enforce numeric and vector invariants during admission or return an explicit loss before
neutral transfer.

**Note.** DrawView.cpp and DrawViewPart.cpp establish the normal property definitions and
direction behavior. The current path permits a hostile value that violates the specification.

## 10. Attachment and assembly

### AT-01. Attachment frame carrier composition

**Question.** How do Placement and AttachmentOffset combine when both are present, and which
property/value is authoritative when repeated?

**Known.** Attachment records retain support, map mode, placement, offset, and an effective frame.
Placement and AttachmentOffset are distinct carriers.

**Conflict.** attachment.rs:27-34 assigns effective_frame = placement.or(offset), so
AttachmentOffset is ignored whenever Placement exists. FreeCAD AttachExtension.cpp applies the
attachment offset separately while computing the attached placement.

**Need.** Implement or specify the producer composition and property cardinality. Do not select
one carrier by presence when both participate in the frame.

**Note.** The current neutral frame is not equivalent to the FreeCAD attachment computation for
objects with both carriers.

### JN-02. Joint carrier runtime types

**Question.** Which runtime type must ObjectToGround and JointType have before their values define
a joint?

**Known.** FreeCAD defines JointType as App::PropertyEnumeration and ObjectToGround as
App::PropertyLinkGlobal. Joint kind cardinality and the out-of-range enumeration rule are
separate from runtime-type admission.

**Conflict.** joint.rs:25-40 selects carriers by name and joint.rs:264-300 reads their child
values without checking the declared runtime type. A wrong-type property with the expected name
can create a grounded or enumerated joint.

**Need.** Enforce exact runtime types for joint carriers and retain or reject wrong-type values
without semantic interpretation.

**Note.** The source JointObject.py property declarations support the type requirement. The
existing JN-01 closure establishes one kind carrier and one Integer value, but not runtime-type
admission.
