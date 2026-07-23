# Siemens NX `.prt`: Open Items

This document records unresolved NX `.prt` byte semantics.

## Parasolid streams

- Compact tombstones whose explicit `(type, xmt)` key does not match a partition entity have no specified target relation. Exact-key tombstones delete that entity; unmatched range or revision semantics remain unspecified.
- Finite parameter domains and branch selection for rank-deficient or terminal type-38/`0x5a` procedural intersections and BLEND_SURF carriers are unspecified. This includes tangential support pairs, offset-surface intersections with multiple branches, blend-section rail domains, and terminal cases where distinct endpoints map to one procedural-curve parameter.
- The relation between graph-only `OFFSET_SURF` constructions and NURBS carriers owned by the topology of the same partition is unspecified when the NURBS surfaces are not geometrically equivalent to the procedural surfaces within the document tolerance. Common signed distance and partition identity do not establish that the NURBS faces are solved caches of those constructions.
- Terminal folds and selection among multiple corrected branches remain unspecified for procedural curves with degenerate support-0 arrays, sentinel-truncated marker-4 plane-support arrays, and NURBS-offset blend spines.
- Full-record layouts for deltas-stream node types outside the topology and procedural families defined in the specification are unspecified.
- The status-framed state tail following the eight-reference deltas BODY revision prefix is unspecified.
- Status-byte placement and complete-record boundaries for deltas-stream NURBS support records types 125–128 and 135–136 are unspecified.
- Delta tag `0x5a` uses the `intersection_data` layout shared with type 38; its canonical later-schema node-type name is unspecified.

## Object model and body composition

- Per-class NX OM field-value serialization is unspecified, including field offsets for feature history, constraints, attributes, and material bindings.
- The geometric roles and coordinate spaces of framed scalar pairs in `SKETCH`, `DATUM_PLANE`, and `DATUM_CSYS` construction payloads are unspecified. Equal scalar pairs do not establish a model-space frame, sketch entity, or constraint relation.
- Offset-only store control blocks outside the zero-prefixed and product-terminated array forms are unspecified.
- The semantic role of the trailing byte in each OM type declaration is unspecified.
- The semantic roles of bytes in each bounded OM field-registry suffix are unspecified.
- Feature-history object-index relations not covered by primary-body writers, Boolean tool consumption, and segment body-image bindings are unspecified.
- Per-operation suppression state and its relation to feature-history records are unspecified.
- Body membership and per-body state for inactive arrangements are unspecified.
- The semantic roles of the extrusion terminal discriminator lane's type indices, fixed counted values, flags, and trailing indices are unspecified.
- The source-curve, target-surface, direction, and combination roles of the ordered `CPROJ` and `CPROJ_CMB` construction references are unspecified.
- The seed, transform, and pattern-control roles of the ordered `Pattern Feature`, `Pattern Geometry`, and `Geometry Instance` construction references are unspecified. The scalar and compact-selector roles in counted pattern rows are unspecified. Equal canonical line labels in distinct pattern and profile blocks do not establish block identity or a seed relation.
- The coordinate and construction roles of the `POINT` header reference, its `02|03` mode, and the two ordered scalar triples in the selected six-scalar lane are unspecified. A target block shared with the following point lane does not identify either triple as the constructed model-space point.
- The drafted-face, neutral-plane, pull-direction, and angle roles of the counted leading indices, four ordered references, and terminal indices and tail in `DRAFT` construction payloads are unspecified.
- The section, guide, continuity, and terminal-control roles of the ordered `SKIN` and `Studio Surface` construction references and their intervening branch groups are unspecified.
- The relationship between plain cached-body streams and their owning features is unspecified.
- The associated `RMFastLoad` per-class entity record layout outside its object-id membership table is unspecified.

## Assembly and material data

- The semantic role of each nonzero `/Root/UG_PART/DisplayJT` outer-index row value is unspecified.
- Assembly occurrence placement semantics are unspecified. `hostglobalvariables` stores expression values, including pattern angles and counts; metric radii and base frames lack defined locations.
- The mapping from child-bound handle sets to distinct assembly occurrences is unspecified.
- The field boundaries and roles of residual `EXTREFSTREAM` tail bytes are unspecified. These bytes are `0x00` padding and small markers interleaved with `e0 + handle:u32` persistent-handle tokens and `0xC0..0xCF + 28-bit-ref` tokens.
- Parasolid SDL/TYSA attribute field-value serialization is unspecified after the type-81 discriminator selects its type-79 class definition. The attribute-definition catalog includes field type codes such as `SDL/TYSA_DENSITY` and `SDL/TYSA_BLEND_ID`, but the class-specific assignment of referenced value records to declared fields remains unspecified.
- Material and appearance bindings to face identity are unspecified.
