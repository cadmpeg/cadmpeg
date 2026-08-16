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

## 6. Product structure

## 7. Assembly joints

## 8. Attachment and assembly

## 9. Persistent graph admission
