# SolidWorks `.sldprt`: Open Items

## Body classification

The schema-33103 sheet discriminator is unresolved. `0x1d/flo2` belongs to the face-connectivity web rather than a sheet region. The discriminator must occur in the body-reachable `0x1b → 0x1f → 0x21 → 0x23` region chain.

- The class-root vector following `index_map_offset` and its relation to body, shell, and face-use heads is unresolved.

## Geometry carriers

- The convention for derived UV pcurves on trimmed B-spline faces is unresolved.
- The carriers for offset, swept, blended, intersection, and spline-on-surface geometry are unresolved.
- The stored topology convention for periodic cylinder and sphere seams is unresolved.
- The relation between bridge/coedge orientation markers and a closed shell's global orientation is unresolved.

## Container metadata

- The cache-cell prefix, fill, and high half of `type_id` have unresolved index-state semantics.
- The variable-length fill after the final tail-directory entry has unresolved index-state semantics.
- The fixed slot-count and boundary grammar for inline entity families outside canonical faces is not defined for all Parasolid schemas.
- The precedence relation between partition and deltas records with the same site, attribute, and sequence is unresolved.

## Auxiliary lanes

- The mapping from B-rep face attributes to DisplayLists triangle ranges is unresolved.

## Design intent

- `ResolvedFeatures` relation families other than point-point, line-line, point-line, horizontal, and vertical distances, angular relations, and circle diameter; operand-to-profile-locus ownership; and relation expressions are unresolved.
- Sketch marker type codes `4..27` are unresolved.
- The ownership link from sketch markers to nested feature-input Parasolid profiles is unresolved.
- Keywords operation families outside the typed neutral feature set are unresolved.
