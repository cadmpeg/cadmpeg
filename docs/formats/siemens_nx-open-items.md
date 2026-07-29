# Siemens NX `.prt`: Open Items

This document records unresolved NX `.prt` byte semantics.

## Parasolid streams

- Compact tombstones whose explicit `(type, xmt)` key does not match a partition entity have no specified target relation. Exact-key tombstones delete that entity; unmatched range or revision semantics remain unspecified.
- Finite branch selection remains unspecified for terminal type-38/`0x5a` procedural intersections with no usable support-parameter lane or paired FIN-carried SP_CURVE witnesses, and for terminal cases where distinct endpoints map to one procedural-curve parameter.
- The relation between graph-only `OFFSET_SURF` constructions and FACE-owned NURBS carriers remains unspecified for periodic supports, differing spline bases, and nonplanar supports whose rational derivative bounds do not certify one regular orientation within the subdivision limit. Common signed distance and partition identity do not establish that the NURBS faces are solved caches of those constructions.
- Terminal folds and selection among multiple corrected branches remain unspecified for procedural curves with degenerate support-0 arrays, sentinel-truncated marker-4 plane-support arrays, and NURBS-offset spines whose graph-only offset supports have no established saved-carrier identity.
- Full-record layouts for deltas-stream node types outside the topology and procedural families defined in the specification are unspecified.
- The field roles of the two consecutive XMT identities in a deltas transmit header are unspecified.
- The field roles within the bounded state tail following the eight-reference deltas BODY revision prefix are unspecified.
- Delta tag `0x5a` uses the `intersection_data` layout shared with type 38; its canonical later-schema node-type name is unspecified.
- The canonical node-type name and value roles of deltas type 45 (`002d`) are unspecified.
- The canonical node-type name and field roles of deltas type 70 (`0046`) are unspecified.
- The semantic role of the leading sentinel reference in deltas type 74 `ATTDEF_LIST` is unspecified.
- The field roles of the five references and `02|04` mode in deltas type 90 `GROUP` are unspecified.
- The canonical node-type name and field roles of deltas type 91 (`005b`) are unspecified.
- The canonical node-type name and field roles of deltas type 101 (`0065`) are unspecified.
- The canonical node-type name and four reference-field roles of deltas type 141 (`008d`) are unspecified.
- The field roles of the count-selected binary64 tail following a deltas `term_use` are unspecified.
- The field roles and owners of deltas tagged-reference lanes are unspecified.
- The field roles and owners of deltas reference/type maps are unspecified.
- The field roles and owners of deltas four-reference state packets are unspecified.
- The field roles and owners of deltas schema reference preambles are unspecified.
- The field roles, marker meaning, and owners of deltas reference-marker packets are unspecified.
- The field roles and marker meaning of deltas type-150 state packets are unspecified.
- The field roles of inline deltas schema declarations are unspecified.
- The field roles and cardinality relations of inline type-12 `BODY` instance state are unspecified.

## Object model and body composition

- Per-class NX OM field-value serialization is unspecified, including field offsets for feature history, constraints, attributes, and material bindings.
- The geometric roles and coordinate spaces of framed scalar pairs in `SKETCH`, `DATUM_PLANE`, and `DATUM_CSYS` construction payloads are unspecified. Equal scalar pairs do not establish a model-space frame, sketch entity, or constraint relation.
- The semantic role of the trailing byte in each OM type declaration is unspecified.
- The semantic roles of bytes in each bounded OM field-registry suffix are unspecified.
- The cross-store relation from a primary feature body field's resolved
  offset-store block to a segment body-image object-index pair is unspecified.
  Feature-history object-index relations not covered by primary-body writers
  and Boolean tool consumption are also unspecified.
- Ownership roles of embedded operation common frames, the roles of their
  exactly framed eight-byte state lanes, and their relation to operation
  suppression are unspecified. Suppression outside the closed active
  configuration output-and-dependency graph is unspecified.
- The target object family and slot roles of the five nullable references at the start of a `DELETE` payload are unspecified.
- Body membership, parameter state, and per-body state for inactive arrangements are unspecified.
- The semantic roles of operation terminal discriminator lanes' type indices, flags, and trailing indices are unspecified.
- The source-curve, target-surface, direction, and combination roles of the ordered `CPROJ` and `CPROJ_CMB` construction references are unspecified.
- The selection roles of the `FSET` selector and its two ordered object-reference groups are unspecified.
- The seed, transform, and pattern-control roles of the ordered `Pattern Feature`, `Pattern Geometry`, and `Geometry Instance` construction references are unspecified. The scalar and compact-selector roles in counted pattern rows are unspecified. The selector groups and trailing references in `Multi Instance Output` lanes have no assigned construction roles. Equal canonical line labels in distinct pattern and profile blocks do not establish block identity or a seed relation.
- The coordinate and construction roles of the `POINT` header reference, its `02|03` mode, and the two ordered scalar triples in the selected six-scalar lane are unspecified. A target block shared with the following point lane does not identify either triple as the constructed model-space point.
- The drafted-face, neutral-plane, pull-direction, and angle roles of the counted leading indices, four ordered references, and terminal indices and tail in `DRAFT` construction payloads are unspecified.
- The section, guide, continuity, and terminal-control roles of the ordered `SKIN` and `Studio Surface` construction references and their intervening branch groups are unspecified.
- The relationship between plain cached-body streams and their owning features is unspecified.
- The associated `RMFastLoad` per-class entity record layout outside its object-id membership table is unspecified.

## Assembly and material data

- The field layouts of `/Root/FastLoad/Structure`,
  `/Root/FastLoad/JT`, and `/Root/UG_PART/LastSavedToggleInfoStream` are
  unspecified.
- The semantic role of each nonzero `/Root/UG_PART/DisplayJT` outer-index row value is unspecified.
- Assembly occurrence placement semantics are unspecified. `hostglobalvariables` stores expression values, including pattern angles and counts; metric radii and base frames lack defined locations.
- The mapping from child-bound handle sets to distinct assembly occurrences is unspecified.
- The field boundaries and roles of residual `EXTREFSTREAM` tail bytes are unspecified. These bytes are `0x00` padding and small markers interleaved with `e0 + handle:u32` persistent-handle tokens and `0xC0..0xCF + 28-bit-ref` tokens.
- Parasolid SDL/TYSA attribute field-value serialization is unspecified after the type-81 discriminator selects its type-79 class definition. The attribute-definition catalog includes field type codes such as `SDL/TYSA_DENSITY` and `SDL/TYSA_BLEND_ID`, but the class-specific assignment of referenced value records to declared fields remains unspecified.
- Material and appearance bindings to face identity are unspecified.
