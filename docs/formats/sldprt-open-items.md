# SolidWorks `.sldprt`: Open Items

## Body classification

The schema-33103 solid and sheet discriminator is unresolved. `0x1d/flo2` belongs to the face-connectivity web rather than a sheet region. The unresolved sheet discriminator must occur in the body-reachable `0x1b → 0x1f → 0x21 → 0x23` region chain.

## Geometry carriers

- The convention for derived UV pcurves on trimmed B-spline faces is unresolved.
- The carriers for offset, swept, blended, intersection, and spline-on-surface geometry are unresolved.

## Container metadata

- The cache-cell prefix, fill, and high half of `type_id` have unresolved index-state semantics.
- The variable-length fill after the final tail-directory entry has unresolved index-state semantics.
- The fixed slot-count and boundary grammar for inline entity families outside canonical faces is not defined for all Parasolid schemas.

## Auxiliary lanes

- The mapping from B-rep face attributes to DisplayLists triangle ranges is unresolved.
- Per-face color carriers can disagree. The precedence rule for appearance overrides is unresolved.
