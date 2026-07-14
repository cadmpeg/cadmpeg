# SolidWorks `.sldprt`: Open Items

## Body classification

The schema-33103 sheet discriminator is unresolved. `0x1d/flo2` belongs to the face-connectivity web rather than a sheet region. The discriminator must occur in the body-reachable `0x1b → 0x1f → 0x21 → 0x23` region chain.

- The class-root vector following `index_map_offset` and its relation to body, shell, and face-use heads is unresolved.

## Geometry carriers

- The derived UV convention for non-isoparametric trims on B-spline faces is unresolved.
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

- `ResolvedFeatures` relation families other than point-point, line-line, point-line, horizontal and vertical distances, angular relations, circular radius and diameter dimensions, unambiguous unary horizontal, vertical, fixed, and fixed arc-angle marker relations, unambiguous binary parallel, perpendicular, tangent, equal, collinear, and concentric marker relations, and unambiguous coincident, horizontal-points, vertical-points, and midpoint marker relations remain unresolved. Operand-to-profile-locus ownership remains unresolved when the marker graph is ambiguous.
- Sketch marker type codes above `27` are unresolved.
- Marker-to-profile correspondence is unresolved when the feature's coordinate sets admit no unique signed-axis transform or a reference marker's linked loci do not identify one profile entity.
- Keywords operation families outside the typed neutral feature set are unresolved.
- Compact extrusion end-spec termination codes other than blind `0`, through-all `1`, and to-face `4` are unresolved. The mapping from an `moSingleFaceRef_w` native component path to a B-rep face is unresolved.
- Compact extrusion Boolean operation values other than inline `00` join and `02` cut are unresolved. Sparse objects without the inline operation remain unresolved outside the `moExtrusion_c` join form `1`, `moICE_c` join form `3`, and `moICE_c` cut forms `1`, `2`, `10`, and `11`.
- The operation carrier for compact `moCombineBodies_c` objects without a corresponding Keywords attribute is unresolved. The mapping from its target and tool component paths to B-rep bodies is unresolved.
- The delete/keep mode discriminator and the mapping from compact `moDeleteBody_c` feature-local body identifiers to B-rep bodies are unresolved.
- The mapping from compact `moCompEdge_c` feature-local edge identifiers and heterogeneous component paths to B-rep edges is unresolved.
- The mapping from `moCompSurfaceBody_c` feature-local component identifiers to B-rep faces is unresolved.
- The reference fields that bind `moSweep_c` general-curve-reference children to neutral paths, composite-profile forms not carried by a unique enclosed planar profile stream, and the Boolean operation discriminator for solid sweep objects without Keywords attributes are unresolved.
