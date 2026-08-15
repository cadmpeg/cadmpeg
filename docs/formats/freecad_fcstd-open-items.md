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

### AR-04. Shared side-entry logical ownership

**Question.** How does the logical byte ledger represent one archive entry referenced by more
than one property or typed payload?

**Known.** EntryRecord.referenced_by retains multiple semantic references while the byte span
has one archive-entry owner.

**Need.** Establish whether typed side entries can be shared. If sharing is valid, keep one byte
span with a many-owner relation. If sharing is invalid for a typed family, reject the conflicting
claims.

**Note.** The representation fix did not establish the producer rule. The closure changes the
ledger shape, not the validity of shared typed references. Reopened after the side-entry closure.

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

**Question.** Does the decoder's topology traversal reproduce the producer's persistent indexed-
map position for every placed occurrence?

**Known.** Persistent element-map names bind to topology occurrences. The element-map root is the
final map node after child maps.

**Conflict.** topology_transfer.rs:1554-1598 assigns indices with a decoder-owned depth-first
walk and a key composed of shape and transform. It does not read a producer index or establish a
FreeCAD or OCCT enumeration rule. Equal shape and transform occurrences can collapse to one key.

**Need.** Establish the producer indexed-map enumeration rule and carry that index through
topology transfer. Preserve distinct persistent occurrences when their source positions differ.

**Note.** The closure corrected counter scope across multiple roots but did not prove that the
replacement walk matches the producer. Reopened after the topology closure.

## 4. Exact-topology transfer

### XT-01. Edge endpoint child selection

**Question.** What child-use cardinality and orientation combinations define the start and end
vertices of normal, closed, degenerate, and malformed edge records?

**Known.** Exact-shape records retain ordered and oriented topology children. Neutral edges
require explicit start and end vertex identities.

**Conflict.** topology_transfer.rs:1691-1721 requires one Forward and one Reversed child and
rejects duplicate orientations. No producer or kernel rule establishes that duplicate
orientation uses are invalid or that this is the complete valid endpoint grammar.

**Need.** Establish the valid endpoint forms and their orientation semantics. Handle each valid
form explicitly and reject only a form that cannot establish both endpoint identities.

**Note.** The closure changed an unverified selection rule into a refusal rule. Reopened after the
topology closure.

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

**Note.** The closure promotes refusal to a format invariant without primary evidence for
multiple representation cases. Reopened after the topology closure.

### XT-03. Non-manifold radial order

**Question.** What source order defines the radial cycle when more than two coedges use the same
edge?

**Known.** Native topology retains ordered child uses and orientations. A neutral coedge has one
radial_next relation.

**Conflict.** topology_transfer.rs:1678-1689 links only two coedges and leaves three or more
self-radial. No producer or kernel rule establishes that the source has no radial order.

**Need.** Establish whether FreeCAD or OCCT supplies a radial order for non-manifold uses. If it
does not, retain unordered incidence or mark radial order unresolved.

**Note.** The closure promotes a neutral fallback to settled source semantics without evidence
for the non-manifold case. Reopened after the topology closure.

## 5. Design projection

### DP-02. Sketch profile seed order

**Question.** Which non-construction entity starts each oriented sketch profile chain?

**Known.** Sketch entities retain persisted source order and native identity. Profile chains must
be deterministic and attributable.

**Conflict.** design.rs:2385-2410 seeds each profile from the first entity in a decoder-owned
BTreeSet. Persisted geometry-list order does not establish that this disconnected-chain seed
rule is producer-defined.

**Need.** Establish the producer-defined seed rule for each disconnected chain and retain the
persisted entity ordinal in the decision.

**Note.** The closure fixed an earlier ordering defect but did not establish the source rule for
disconnected profiles. Reopened.

### DP-03. Sketch profile junction ambiguity and tolerance

**Question.** What endpoint tolerance connects two sketch entities, and what happens when more
than one unused entity meets the current endpoint?

**Known.** Constraints and persisted geometry can produce coincident endpoints. A neutral profile
chain asserts one ordered continuation and orientation at every junction.

**Conflict.** design.rs:2474-2499 supplements constraints with coordinate matching.
endpoints_match_by_roundoff at design.rs:2593-2603 uses 64 machine epsilons scaled by coordinate
magnitude. No producer or kernel rule establishes this numeric boundary or the full admissible
profile topology.

**Need.** Establish endpoint equivalence and the admissible profile topology. An ambiguous
junction must use constraint identity, an explicit source-order rule, or an attributable refusal.

**Note.** The closure adds ambiguity handling and a scale formula, but the boundary remains a
decoder policy without producer evidence. Reopened.

### DP-05. Dependency-cycle ordinal fallback

**Question.** What neutral projection applies when feature dependencies, parents, or expressions
form a cycle?

**Known.** The native graph retains cycles. The neutral graph must use a stable maximal subset whose
targets precede their consumers, or carry an explicit blocking loss.

**Conflict.** design.rs:679-688 marks all remaining objects cycle-affected, assigns ordinals by
source order, and design.rs:450-456 removes edges whose targets are not earlier. The specification
now records this policy, but no producer cycle projection establishes that source order and edge
discard are the correct neutral result.

**Need.** Define a cycle projection that is stable and preserves the maximal admissible subset, or
refuse with an explicit loss. Do not source-order a cycle and silently discard its edges.

**Note.** Native retention and the blocking feature.cyclic-history loss are a safety improvement.
They do not establish the neutral relation. Reopened after the history closure.

### DP-07. Sketch geometry carrier tag and runtime name are not cross-checked

**Question.** Must a Geometry element's declared runtime name agree with its sole eligible carrier
child tag before neutral geometry dispatch?

**Known.** PropertyGeometryList.cpp writes the Geometry type attribute from the geometry runtime
type and serializes the geometry value beneath that element. The specification requires exact
runtime dispatch and native retention for unknown or conflicting carriers.

**Conflict.** design.rs:1073-1117 accepts one non-metadata child, takes the Geometry type
attribute in preference to the child tag, and passes that child's attributes to the selected
geometry parser. It does not reject a known type paired with a different child tag.

**Need.** Validate the runtime-name and carrier-tag pair before semantic parsing. Retain a
conflicting pair as native or reject it.

**Note.** A Geometry declared as Part::GeomLineSegment with one Circle child can be parsed through
the line path when its attributes are parseable; the child grammar does not protect the dispatch.

### DP-08. Sketch placement silently defaults incomplete components

**Question.** Which placement components are mandatory before a sketch frame is transferred to
neutral geometry?

**Known.** A persisted placement supplies the sketch origin and basis after quaternion
normalization. Invalid geometry remains attributable native data.

**Conflict.** design.rs:1416-1460 selects Placement or AttachmentOffset without a runtime-type
gate, defaults missing or unparsable Px, Py, Pz, and quaternion components, and
design.rs:1463-1467 turns a zero quaternion into the canonical basis. The fallback creates a
neutral frame for incomplete placement data without a finding or native-only decision.

**Need.** Require the complete placement carrier and finite nonzero rotation, or retain the
affected sketch geometry native with an explicit loss.

## 6. Product structure

### PR-03. Product named carrier runtime types and cardinality

**Question.** Which exact runtime type and value cardinality must each named product carrier have
before its value enters a neutral occurrence?

**Known.** FreeCAD Link.h declares LinkedObject and copy-on-change links as App::PropertyLink,
LinkTransform and LinkCopyOnChangeTouched as App::PropertyBool, Scale as App::PropertyFloat,
ScaleVector as App::PropertyVector, VisibilityList as App::PropertyBoolList, ElementCount as
App::PropertyInteger, and ElementList as App::PropertyLinkList. Placement carriers use
App::PropertyPlacement.

**Conflict.** product.rs:40-47 and 89-104 select Group, LinkedObject, LinkTransform, copy-on-
change links, scalar flags, Scale, VisibilityList, and ElementList by name. product.rs:736-775
reads the first link or parseable value without checking the declared runtime type or all required
cardinalities. A LinkList with one target can populate scalar LinkedObject; a wrong-type
LinkTransform can select LinkPlacement; and a wrong-type list or scalar can populate array fields.

**Need.** Gate every named carrier by the exact producer runtime type, enforce its value and link
cardinality, and retain or reject wrong-type carriers without neutral interpretation.

**Note.** PR-01's AssemblyLink registry entry and PR-02's LinkCopyOnChange type gate are sound.
The remaining named carriers are not covered by those closures.

## 7. Assembly joints

### JN-03. Joint connector reference runtime type and cardinality

**Question.** Which runtime type and object cardinality must Reference1 and Reference2 have before
their targets become joint operands?

**Known.** JointObject.py declares both references as App::PropertyXLinkSub. Each connector has
one target with an ordered subelement path. Joint validation expects two connector frames for a
non-grounded joint.

**Conflict.** joint.rs:79-97 calls links by name without checking the carrier type or enforcing
one target per connector. The persistence parser accepts App::PropertyXLinkSubList and produces
multiple LinkTarget values, while lib.rs:608-625 checks frame count and target existence but not
reference count or carrier type.

**Need.** Enforce the exact connector link family and one target per Reference1 and Reference2.
Preserve an explicit null or external target according to the link contract without creating
extra operands.

**Note.** A hostile XLinkSubList with two targets in Reference1 produces more than two neutral
operands while the two-frame validation still passes.

### JN-04. Joint scalar parameter runtime types

**Question.** Which runtime type must each named joint scalar and boolean parameter have before its
value enters the neutral joint?

**Known.** JointObject.py declares Angle and angle limits as App::PropertyAngle, distances and
length limits as App::PropertyLength, and enable, detach, and suppression flags as
App::PropertyBool. Each named parameter has at most one root value.

**Conflict.** joint.rs:99-127 selects parameters by name, and scalar_parameter at
joint.rs:325-333 reads any single value attribute without checking the runtime type or value tag.
A PropertyString or PropertyInteger with a parseable value can populate a neutral angle, length,
or boolean field.

**Need.** Enforce the exact runtime type and root value tag for every named joint parameter before
neutral transfer.

## 8. Attachment and assembly

### AT-02. Attachment support and map-mode carrier grammar

**Question.** Which exact runtime types and value grammar identify Support and MapMode?

**Known.** AttachExtension.h declares the support as App::PropertyLinkSubList, MapMode as
App::PropertyEnumeration, and AttachmentOffset as App::PropertyPlacement. PropertyEnumeration
serializes one Integer index and optional CustomEnumList metadata; Attacher.cpp supplies the mode
name order.

**Conflict.** attachment.rs:27-39 selects Support and MapMode by name only. Support links have no
runtime-type gate. property_text reads the first text-like value, so a normal enumeration's
Integer index is retained as a numeric string rather than its mode name; a wrong PropertyString
can also populate map_mode.

**Need.** Enforce the accepted support link families and MapMode enumeration grammar, including
index-to-name mapping, cardinality, and out-of-range behavior.

**Note.** The AT-01 composition closure is source-backed. It does not establish carrier typing or
map-mode value semantics.
