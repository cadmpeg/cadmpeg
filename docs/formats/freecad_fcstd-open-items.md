# FreeCAD .FCStd: Open Items

This document records unresolved FreeCAD .FCStd format questions. The specification records
settled byte semantics and invariants.

Each item has an identifier and these fields:

- Question
- Known
- Need
- Conflict
- Note

## 3. Persistent topology identity

## 4. Exact-topology transfer

### ET-01. Competing face pcurve representations

**Question.** When an edge has two or more curve-on-surface representations whose surface identity
and composed location match one face use, which representation supplies that use's pcurve?

**Known.** One edge can retain several ordered representations. A kind-3 closed-surface
representation contains a defined primary/secondary pair, selected by edge-use orientation. This
paired form is distinct from two separate representations that both match the same face use. For
exact 3D curves, equal curve identity, location, and parameter range establish equivalence, and a
conflicting repeat is invalid. No corresponding equivalence rule is specified for separate matching
pcurve representations.

**Need.** Determine the OCCT restore and traversal semantics that associate a curve-on-surface
representation with a particular face use. Define the representation identity and equality fields
that either select one representation without order dependence or make competing matches invalid.

**Conflict.** The current neutral projection selects the first matching representation in serialized
order. Therefore two source records with the same set of matching representations in a different
order can produce different neutral pcurves.

**Note.** Keep the representation-selection rule separate from pcurve parameter normalization and
from the primary/secondary rule inside one closed-surface representation.

## 5. Design projection

### DP-01. Sketch profile identity and connectivity

**Question.** Which FreeCAD semantics define the membership, direction, starting entity, and order of
each profile projected from a sketch?

**Known.** Sketch geometry and constraints persist entity identity, entity order, construction state,
and explicit coincident loci. They do not persist a neutral profile-chain record. Equal endpoint
coordinates alone do not state whether two unconstrained entities belong to one intended profile.
Disconnected components also have no persisted neutral seed field.

**Need.** Define which FreeCAD objects or operation inputs establish profile membership and ordering.
For cases that require a CADIR-derived profile, specify the admissible derivation and the result for
unconstrained equal endpoints, branches, overlaps, closed chains, and disconnected components.

**Conflict.** The current projection joins unconstrained endpoints with a decoder-owned floating-point
roundoff threshold and starts each derived profile at the lowest unused geometry ordinal. These rules
can create connectivity and ordering that are not explicit FreeCAD document semantics.

**Note.** Endpoint matching and seed selection are decoder policy, not FCStd format knowledge. Preserve
the exact sketch entities and constraints independently of any neutral profile projection.

### DP-02. Feature dependency admission

**Question.** Which object properties and document relations define recompute dependencies between
FreeCAD features?

**Known.** Explicit object-dependency records, body membership, expression references, and typed
operation operands each carry distinct relations. Property links can target either earlier or later
objects. Source declaration order does not by itself state whether an arbitrary property link is a
construction dependency.

**Need.** Define dependency semantics from FreeCAD property types, runtime-object behavior, extension
metadata, and explicit dependency records. State how forward links, custom properties, extension-owned
links, and links that are references but not construction inputs affect the neutral feature graph.

**Conflict.** The current projection treats an enumerated set of property names as dependencies and
admits other property-link targets only when they occur earlier in source order. A genuine dependency
with another name can be omitted, while an earlier non-dependency link can be admitted.

**Note.** Stable ordinal assignment is a separate CADIR policy. It must not determine whether a native
relation is a semantic feature dependency.

### DP-03. External sketch-geometry selector cardinality

**Question.** Can one `ExternalGeometry` link select multiple subelements, and how does each cached
`ExternalGeo` geometry record identify the selected subelement or selector group?

**Known.** `App::PropertyLinkSubList` retains an ordered object target and ordered subelement selectors.
A cached external-geometry record carries one reference key. The neutral external entity retains all
subelement selectors from its matched link.

**Need.** Define the exact FreeCAD mapping between link-list entries, their subelement lists, cache
reference keys, and cached geometry records. State whether a link entry must contain exactly one
subelement or whether one entry can own several independently keyed cache records.

**Conflict.** The current cache-key construction uses only the first subelement of each link without
enforcing singleton cardinality. Later selectors cannot participate in cache matching even though
they remain attached to the neutral external entity.

**Note.** Do not replace this question with a first-selector policy. Either prove singleton cardinality
as a FreeCAD invariant or represent the complete multi-selector mapping.

## 6. Semantic annotations

## 7. TechDraw projection

## 9. Assembly joints

## 10. Attachment and assembly

### AA-01. Product part-number carrier semantics

**Question.** How do `PartNumber` and the built-in `Id` property contribute to product identity and BOM
part number for each supported FreeCAD product runtime?

**Known.** Both names can occur on part and assembly objects, and the native graph retains them as
separate typed properties. An empty value is distinct from an absent carrier. The document does not
persist a separate field that declares one carrier to be the neutral CADIR part number.

**Need.** Define the FreeCAD application semantics of both properties by runtime type, including
inheritance, overrides, empty values, generated assembly objects, and user-added properties. Then
define the neutral mapping without using carrier validity as an implicit precedence rule.

**Conflict.** The current projection gives a nonempty `PartNumber` precedence and otherwise uses `Id`
for selected part runtimes. This is a CADIR-selected metadata policy rather than a decoded format
discriminator.

**Note.** Preserve both native properties regardless of the neutral mapping. Product identity and BOM
display policy are separate concepts.

### AA-02. Link-array cardinality without `ElementCount`

**Question.** What occurrence count does FreeCAD restore when an array link has no `ElementCount`
property but contains one or more array-valued carriers?

**Known.** A present `ElementCount` supplies an explicit count. Placement, scale, visibility, and
element-object arrays each carry their own lengths, and inconsistent nonempty lengths are invalid.
Scalar link state remains distinct from per-element array state.

**Need.** Define the producer and restore semantics for absent, zero, and positive `ElementCount`,
including which array carrier can establish cardinality and whether absence denotes a scalar link,
legacy array encoding, or a default property value.

**Conflict.** The current projection infers an absent count from the longest populated array and uses a
minimum count of one. The maximum-array rule is decoder recovery policy unless it matches FreeCAD's
restore semantics.

**Note.** Keep count restoration separate from validation that all populated arrays have a consistent
length.

## 11. Persistent graph admission
