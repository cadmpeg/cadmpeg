# Siemens NX `.prt`: Open Items

This document records NX `.prt` semantics that the format specification does not yet define.

## Parasolid streams

- The mapping from a six-byte tombstone record to the partition entity range that it deletes is unspecified. This mapping determines the surviving face set of an active body.
- The derivation of finite trim and parameter ranges for reconstructed carriers is unresolved. Select the intended branch for offset-surface intersections with multiple branches, derive blend-section rails from the decoded relation, reconcile decoded edge bounds with carrier parameter ranges, and handle terminal cases where distinct edge endpoints map to one curve parameter.
- Surface-surface continuation remains unspecified for procedural curves with degenerate support-0 arrays, sentinel-truncated marker-4 plane-support arrays, and NURBS-offset blend spines.
- Full-record layouts for deltas-stream node types outside the decoded topology and procedural families are unspecified.
- The assignment of `ext11` CHART_s parameters `p3..p6` to the two support surfaces is unspecified.
- Confirm the `0x5a` delta-intersection tag against a later-schema Parasolid Node Types table. Its decoded layout is the `intersection_data` layout shared with type 38; the unresolved semantic is the canonical later-schema name.

## Object model and body composition

- NX OM entity records lack a defined per-class field-value serialization. The format does not yet define field offsets for feature history, constraints, attributes, or material bindings.
- The feature-history Boolean operand bindings and composition order across partition and deltas body pairs are unspecified.
- The relationship between plain cached-body streams and their owning features is unspecified.
- `RMFastLoad` object IDs identify active-body membership, but the associated per-class entity record layout is unspecified.

## Assembly and material data

- Assembly occurrence placement semantics are unspecified. `hostglobalvariables` stores expression values, including pattern angles and counts; metric radii and base frames lack defined locations.
- The occurrence-handle to child-`.prt` binding is unspecified.
- The framing of residual `EXTREFSTREAM` tail bytes is unresolved. The tail otherwise decodes as `e0 + handle:u32` persistent-handle tokens and `0xC0..0xCF + 28-bit-ref` tokens; the residual bytes are `0x00` padding and small interleaved markers.
- Parasolid SDL/TYSA attribute instance serialization is unspecified. The attribute-definition catalog includes class names, class IDs, and field type codes such as `SDL/TYSA_DENSITY` and `SDL/TYSA_BLEND_ID`.
- Material and appearance bindings to face identity are unspecified.
