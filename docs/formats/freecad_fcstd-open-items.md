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

## 11. Persistent graph admission
