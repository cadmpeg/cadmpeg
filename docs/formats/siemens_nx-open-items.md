# Siemens NX `.prt`: Open Items

This document records unresolved NX `.prt` byte semantics.

## Parasolid streams

- Compact tombstones whose explicit `(type, xmt)` key does not match a partition entity have no specified target relation. Exact-key tombstones delete that entity; unmatched range or revision semantics remain unspecified.
- Finite parameter domains and branch selection for type-38/`0x5a` procedural intersections and BLEND_SURF carriers are unspecified. This includes offset-surface intersections with multiple branches, blend-section rail domains, and terminal cases where distinct endpoints map to one procedural-curve parameter.
- Surface-surface continuation remains unspecified for procedural curves with degenerate support-0 arrays, sentinel-truncated marker-4 plane-support arrays, and NURBS-offset blend spines.
- Full-record layouts for deltas-stream node types outside the topology and procedural families defined in the specification are unspecified.
- The assignment of `ext11` CHART_s parameters `p3..p6` to the two support surfaces is unspecified.
- Delta tag `0x5a` uses the `intersection_data` layout shared with type 38; its canonical later-schema node-type name is unspecified.

## Object model and body composition

- Per-class NX OM field-value serialization is unspecified, including field offsets for feature history, constraints, attributes, and material bindings.
- The semantic role of the trailing byte in each OM type declaration is unspecified.
- The feature-history Boolean operand bindings and composition order across partition and deltas body pairs are unspecified.
- The relationship between plain cached-body streams and their owning features is unspecified.
- The associated `RMFastLoad` per-class entity record layout outside its object-id membership table is unspecified.

## Assembly and material data

- Assembly occurrence placement semantics are unspecified. `hostglobalvariables` stores expression values, including pattern angles and counts; metric radii and base frames lack defined locations.
- The occurrence-handle to child-`.prt` binding is unspecified.
- The field boundaries and roles of residual `EXTREFSTREAM` tail bytes are unspecified. These bytes are `0x00` padding and small markers interleaved with `e0 + handle:u32` persistent-handle tokens and `0xC0..0xCF + 28-bit-ref` tokens.
- Parasolid SDL/TYSA attribute instance serialization is unspecified. The attribute-definition catalog includes class names, class IDs, and field type codes such as `SDL/TYSA_DENSITY` and `SDL/TYSA_BLEND_ID`.
- Material and appearance bindings to face identity are unspecified.
