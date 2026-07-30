# SolidWorks `.sldprt`: Open Items

## Body classification

- The class-root vector following `index_map_offset` is unresolved. Body, shell, and face-use heads bind through the disc-keyed layouts and the cluster-key chain form; sites whose list heads satisfy neither shape remain unresolved.
- Deltas-stream face entities outside a multi-chain site's partition intervals have no resolved body assignment.
- The structural discriminator separating partition faces superseded by a deltas change set from partition faces the change set retains is unresolved; roster membership, roster-entry record presence, and the owning loop's or owner entity's record presence do not determine supersession. Until the discriminator is resolved, deltas bridges do not extend a partition's face membership.

## Geometry carriers

- The derived UV convention for non-isoparametric trims on B-spline faces is unresolved.
- A `00 26` intersection composite whose chart, terminator, and support-UV witnesses are absent or mutually inconsistent in the retained streams has no resolved curve; where those witnesses are stored is unresolved. Untyped edge curve attributes owned by surface records rather than curve carriers are unresolved.
- The carriers for offset, non-constant-radius blend, surface-intersection, and spline-on-surface surface geometry are unresolved.

## Container metadata

- The cache-cell prefix, fill, and high half of `type_id` have unresolved index-state semantics.
- The variable-length fill after the final tail-directory entry has unresolved index-state semantics.
- The fixed slot-count and boundary grammar for inline entity families outside canonical faces is not defined for all Parasolid schemas.
- The precedence relation between partition and deltas records with the same site, attribute, and sequence is unresolved.

## Auxiliary lanes

- The mapping from B-rep face attributes to DisplayLists triangle ranges is unresolved.

## Design intent

- The native identities of optional classless feature-manager nodes are unresolved. Their source IDs are allocation positions and shift when earlier nodes are absent; source ID alone does not distinguish the lights-and-cameras container, scene lights, design binder, comments, body folders, materials, equations, notes, sensors, favorites, history, selection sets, markups, and exploded views.
- The native storage carrying the equation manager's angular-unit mode is unresolved. Trigonometric expressions with explicit angular units evaluate; bare numeric trigonometric operands remain unevaluated because their degree-or-radian interpretation is document state.
- The native document-property namespace used by equations is unresolved. Configuration attributes retain configuration-local properties, but no resolved carrier binds default file properties or establishes per-configuration property lookup for `property@configuration@part` operands.
- `ResolvedFeatures` offset-edge marker relations and the top, bottom, left, and right arc-cardinal marker relations have no neutral invariant yet. Point-point, line-line, point-line, horizontal and vertical distances, angular relations, circular radius and diameter dimensions, unary horizontal, vertical, fixed, fixed-sweep arc-angle, and fixed-sweep ellipse-angle relations, binary parallel, perpendicular, tangent, equal, collinear, concentric, and coradial relations, and coincident, merge-points, horizontal-points, vertical-points, midpoint, analytic at-intersection, and point-locus symmetric relations project when their operands resolve uniquely and satisfy the solved geometry. Operand-to-profile-locus ownership remains unresolved when the marker graph identifies handles without uniquely identifying their geometric loci.
- Neutral invariants and operand roles for codes `29..32`, `36..41`, and `43..85` remain incomplete. All native identities and the numeric taxonomy through `85` are defined. Codes above `85` are unresolved native extensions.
- Marker-to-profile correspondence is unresolved when the feature's coordinate sets admit no unique signed-axis transform or a reference marker's linked loci do not identify one profile entity.
- The construction-state discriminator for dimensioned circular geometry absent from a selected profile stream is unresolved.
- Model-space placement of marker-only profile objects remains unresolved when no object-local, immediately contextual, or unique lane-wide compact reference-plane record exists.
- Keywords operation families outside the typed neutral feature set are unresolved.
- The u32 endpoint selector of the up-to-vertex `3` edge-endpoint point-reference form has no neutral semantics; the edge-endpoint reference projects with the endpoint retained natively. Codes above `9` have no neutral projection. Second-direction end-spec codes other than `0` and `1`, second-direction end specs whose first-direction code is not `0`, `1`, or `9`, and end-spec-shaped records whose +18 word carries a reference child rather than a termination code are unresolved. Reconciliation between generated feature-local faces selected by `moSingleFaceRef_w` paths and faces that survive in the final B-rep is unresolved.
- The discriminator separating the joining minority of `moICE_c` form-`11` objects from the subtracting majority is unresolved. Inline operation bytes other than `moExtrusion_c` `00` and `moICE_c` `02` and sparse objects outside the known form codes remain unresolved.
- Reconciliation between generated feature-local bodies selected by compact `moCombineBodies_c` target and tool paths and bodies that survive in the final B-rep is unresolved.
- Reconciliation between compact `moDeleteBody_c` regeneration-input-local body identities and bodies that survive in the final B-rep is unresolved.
- Reconciliation between generated feature-local edges selected by entry-form `moCompEdge_c` paths and edges that survive in the final B-rep is unresolved. Compact-ID edge vectors remain unresolved.
- Reconciliation between generated feature-local faces selected by `moCompSurfaceBody_c` paths and faces that survive in the final B-rep is unresolved.
- General-curve-reference forms without a component-profile source record or an immediately preceding uniquely resolved profile feature remain unmapped to sketch or B-rep geometry. Composite sweep-profile forms not carried by a unique enclosed planar profile stream or an immediately following uniquely resolved profile feature and compact Boolean operation codes other than join code `15` are unresolved.
- The discriminator between the eight- and nine-scalar compact line-reference forms is unresolved when both final-triple interpretations are unit vectors.
