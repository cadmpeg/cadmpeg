# CATIA V5 `.CATPart` coverage

This document applies the cumulative support ladder to the CATIA V5 reader. It
records implementation coverage and verification gates. Byte semantics belong
in [catia.md](catia.md); unresolved byte meanings belong in
[catia-open-items.md](catia-open-items.md).

## Envelopes

The reader recognizes `V5_CFV2` part documents and classifies their geometry
storage as `standard_nested`, `fbb_only`, `zero_entity`, `e5_stream`,
`float_packed_inner_no_fbb`, `inner_no_directory`, or `unknown`.

The current primary envelope is `standard_nested`: a nested `V5_CFV2` stream
with an FBB spine and the standard edge-table delimiter. Its implementation
score is L2 claimed. The other recognized geometry layouts form separate
envelopes and currently score L1 claimed. `unknown` is an inspection and
retention envelope and does not inherit geometry support.

These envelopes are not closed release bands. Supported CATIA release bounds,
required and optional container segments, and the admitted carrier, topology,
appearance, and feature-family matrices have not been fixed. Claims above the
current scores require those matrices and representative fixtures.

## Cumulative gates

| Level | Required evidence | Current result | Remaining gate |
| --- | --- | --- | --- |
| L0 | `V5_CFV2` detection; part-kind and layout classification; bounded metadata; exact preview extraction when stored | Pass in implementation | Representative release-band fixtures and explicit preview-present/absent coverage |
| L1 | Nested-stream, directory, segment, extent, and record navigation; layout dispatch; external-reference and embedded-asset enumeration; named undecoded content | Claimed across recognized layouts | Close the release/layout envelope and verify every admitted container combination |
| L2 | Placed points; analytic curves and surfaces; NURBS; correct units, parameterization, and model-space placement throughout one envelope | Claimed for `standard_nested`; incomplete for the other layouts | Close every admitted carrier branch, persistent carrier binding, parameter chart, placement, and unit path |
| L3 | Connected bodies through vertices with exact ownership, orientation, trimming, placement, and transforms; structurally valid topology throughout one envelope | Incomplete | Resolve every admitted face group, endpoint registry, edge incidence, loop orientation, body/shell ownership, and cross-group membership case without topology gauges standing in for source semantics |
| L4 | Ordered feature operations with complete profiles, directions, extents, outputs, dependencies, and solved sketch geometry | Incomplete | Decode construction order and dependencies, sketch membership and geometry, and complete operands for every admitted operation family |
| L5 | Every admitted carrier and topology case; typed mainstream bodies throughout; body and face colors with source ownership and precedence | Incomplete | Close L2/L3 matrices, appearance bindings, and precedence, then demonstrate zero shape-domain loss across the envelope |
| L6 | Complete sketch constraints, dimensions, parameters, expressions, configurations, feature semantics, and re-derivable history | Incomplete | Complete relation and constraint incidence, parameter values and types, feature families, configuration state, and history replay coherence |

## Implemented slices above the score

- Stream-directory inspection retains every extent's raw flags word in logical
  extent order.
- Structurally complete object graphs retain design objects, ordered fields,
  exact field classes, definition-bound values, and inter-object reference
  occurrences.
- Complete two-definition value chains retain the repeated schema selector,
  second role definition, catalog-resolved selected value, and structural
  ownership. Each design object retains its chain-value entity identities in
  field order. Finite and unset evaluations, atoms, controls, separators, and
  nested schema selectors are counted independently.
- Legacy typed relations retain their `body` and `param` role selectors and
  exact parameter identities when those selectors close within one identity
  run.
- Complete numeric parameters and formula relations transfer when their type,
  owner, evaluation state, expression, and dependency identities resolve
  exactly. Typed unset inputs transfer independently while preventing formula
  evaluation. Every transferred numeric parameter retains its canonical
  `LENGTH`, `ANGLE`, `Real`, or `Integer` value type independently of whether
  an evaluated value exists.
  Placeholder-state, Boolean-prefixed parser-version, unprefixed
  parser-version, and opened Boolean parser-version relation-expression
  framings retain their source expressions and typed signatures. Parser-version
  expressions remain native when their formula-instance incidence does not
  resolve.
  Parameters structurally contained by a transferred feature retain that
  feature ownership. Structurally owned sketches are ordered after their
  transferred sketch ancestors independently of field serialization; cyclic
  owner sets remain unparented. Child features and parameters share one
  object-field-ordered source-content sequence; other parameters remain
  document-scoped.
  A design object whose complete empty declaration population has positionally
  paired definition values selecting one unanimous `CircPattern` or
  `RectPattern` entry transfers one circular- or linear-pattern feature
  identity and consumes those declarations. Declaration field classes remain
  independent operation roles. Seeds, directions, counts, spacing, angles,
  field roles, and outputs remain unresolved. Coverage counts the transferred
  circular and linear feature identities separately from their consumed
  declaration records.
  Typed `Boolean` and `String` parameters transfer when their complete
  evaluation production is unset. Binary64 evaluation productions do not
  transfer as values of either type.
  Legacy named scalar packets also transfer with either a finite or unset
  evaluation when a unique acyclic descriptor chain resolves their literal
  numeric type. A uniquely bound zero-input legacy relation replaces a finite
  scalar literal when its typed evaluation agrees exactly and supplies the
  expression of an unset parameter when its result type agrees.
  Arithmetic evaluation retains length and angle exponents through intermediate
  products, quotients, extrema, trigonometric calls, absolute values, and
  square roots.
  Complete constraint-range productions are counted separately as dimension or
  complex-constraint ranges, finite or unset evaluations, and structurally
  owned or owner-unresolved records. These classifications do not establish
  constraint identity, operands, or sketch incidence.
- Owner-bound `PRTSketch` class fields transfer one neutral planar
  sketch identity and its linked ordered sketch-history feature independently
  of unresolved geometry payloads. Complete empty declaration records are
  consumed separately. Structural ownership among transferred sketches
  supplies feature containment. Exact principal-plane declarations resolve the
  corresponding origin frame. Design objects consisting entirely of one exact
  empty principal-plane declaration class transfer the corresponding built-in
  reference-plane history node. Plain `Sketch` fields are properties and do not
  declare sketch identities.
- Several non-primary envelopes transfer connected topology or exact analytic,
  NURBS, and procedural carrier subsets. These are extras until every
  cumulative gate in one closed envelope passes.
- Zero-entity face-local support occurrences with complete lifted endpoint
  tapes form radial endpoint-pair candidates when two occurrences have one
  reciprocal unordered model-space endpoint match and the surrounding
  face-incidence partition is unambiguous. Repeated endpoint pairs additionally
  require one reciprocal bounded-curve midpoint match. This relation does not
  assert curve coincidence: distinct curved supports may share the retained
  witnesses. Candidate endpoint
  coordinates form geometric endpoint-locus candidates only when each tolerance
  component is a complete pairwise clique; ambiguous tolerance chains remain
  unresolved.
- Complete consolidated cone-face chart records retain their reference-and-
  control program, angular scale, and cone half-angle before the program's
  higher-level roles resolve. Consolidated parameter-space points retain all
  four stored prefix selectors with their UV, station-plus-UV, or five-scalar
  payload. A cone-face chart binds its immediately following complete
  parameter-space point run in serialized order; a mixed class-`0x18` run
  remains unbound atomically.
- A consolidated revolution whose profile interval selects exactly one
  consolidated circle transfers a placed analytic directrix and neutral
  surface-of-revolution construction. Missing and multiply matching intervals
  remain unbound atomically.
- Every consolidated line profile transfers its placed signed-distance
  analytic line carrier and stored parameter interval.
- A standard binary32 cylinder, cone, sphere, or torus face carrier is refined
  to its complete consolidated binary64 frame when quantization selects exactly
  one same-family record. One exact carrier may refine repeated face carriers;
  missing and multiply matching records leave the standard carrier unchanged.
  A layout-`0x62` cylinder retains the exact redundant origin of its partial
  circumferential interval and rejects a tail that disagrees with the interval
  midpoint and radius.
  Object-stream planes retain both active chart intervals and require their
  complete unit frame and fixed chart scalars. Object-stream cylinders retain
  independent geometric radius and angular gauge, active circumferential and
  axial intervals, and the full-turn chart origin; pcurve lifting uses the
  stored gauge rather than assuming radius-scaled U.
  Object-stream revolution surfaces require their complete reference-width
  dependent payload with its single-reference cardinality, retain the stored
  profile and angular intervals, and validate the right-handed frame, fixed
  controls, positive angular gauge, and exact half-turn relation. Their line
  and arc profiles require complete unit frames, fixed metric controls, ordered
  intervals, and centered periodic arc domains. A revolution transfers only
  when its profile interval equals the referenced profile's complete interval.
  Its exact NURBS cache uses the stored ranges directly rather than deriving a
  patch from available pcurves.
  Consolidated and object-stream tori retain independent active major- and
  minor-angle intervals and their centered full-turn chart domains.
  Object-stream tori additionally require the complete lead, right-handed
  orthonormal frame, and zero tail; malformed frame, tail, or range/domain
  relations reject the complete record.
  Consolidated and object-stream cones retain their active azimuth interval,
  centered full-turn domain, slant interval, and fixed chart-tail scalars.
  Object-stream cones additionally require the complete lead and signed
  orthonormal frame; neutral pcurve transfer preserves its handedness.
  Malformed frame or chart relations reject the complete record.
  A consolidated sphere retains active azimuth and latitude intervals and
  validates its repeated radius and redundant centered-domain origin exactly.
  An object-stream sphere retains independent geometric and construction
  radii, active azimuth and latitude intervals, and its stored construction
  chart origin. The chart origin must agree with the construction-scaled
  centered azimuth origin within two binary64 relative epsilon.
- The freeform fallback transfers complete consolidated cylinder, cone, sphere,
  and torus records as placed analytic surface carriers independently of
  unresolved topology ownership.
- Zero-entity surface carriers retain their complete face-local `21xx` support
  tapes. Every occurrence keeps its framed local slot, record family, and
  complete clamped `2118`, `2145`, `2171`, `2172`, `2191`, `2199`, `219f`,
  `21d6`, or `21e8` parameter-space NURBS curve, including rational `2199`
  weights, together with its inline UV endpoint pair. Plane pcurves lift
  affinely to exact model-space NURBS carriers. Constant-coordinate pcurves on
  cylinder, cone, torus, and NURBS surfaces retain their exact analytic or
  contracted NURBS carriers.
  Non-isoparametric affine cone pcurves retain exact conical-helix
  constructions. Other complete pcurves transfer as cacheless one-sided
  parametric surface-curve constructions over their exact active NURBS domain.
  Each exact model carrier retains the two parameters
  corresponding to the stored UV endpoints and the model-space point at the
  midpoint of its bounded pcurve parameter interval. The independent `5e1a`
  edge-stride registry retains its fixed
  tagged-one prefix and closed `[T,X,Y,T−1,T−2]` allocation tuples. The `2569`/`0638`
  positional-use and counted `05xx` vertex-incidence registries retain
  one-based global record ordinals and their exact nonzero allocation lanes.
  Each counted vertex-incidence record binds its immediately following `5d06`
  vertex-owner record. The terminal
  `6142`/`6006`/`6508` hierarchy retains the
  complete descending face-allocation roster and binds it through one shell and
  body; the `6142` logical extent includes its full continuation beyond the
  nominal frame. Supports with inline UV endpoints lift those endpoints through
  their exact plane, cylinder, circular-cone, torus, or NURBS owner carrier into
  model space.
  A complete `5fxx` face roster binds positionally to those support tapes and
  retains each face's counted allocations and ordered loop terminals.
  Complete `62xx` loop rosters bind to those terminals and retain alternating
  logical-member and typed-reference lanes, loop class, and absolute member
  senses. Each loop member binds to the unique face-local support occurrence
  whose slot equals the loop terminal minus that member; the complete
  face-local binding is atomic and retained as support-record ordinals. A loop
  retains its complete sense-oriented model endpoint tape when every support
  lifts directly or exactly one missing pair is bounded by lifted neighbors
  and every cyclic join closes within the format tolerance. Matching occurrence
  endpoint pairs establish face-incidence components when each occurrence has
  one reciprocal endpoint match. Ambiguous endpoint groups require one
  reciprocal bounded-curve midpoint match. Matching groups partition by those
  components, and every two-occurrence partition retains one radial
  endpoint-pair candidate. The midpoint disambiguates repeated endpoint pairs;
  the candidate does not establish curve coincidence or a physical edge. Every
  in-range odd-lane typed reference retains its selected global record identity
  atomically for the loop.
  In the object stream, full-pcurve endpoint loci are geometric fallback
  candidates only. Conflicting candidates for one edge are discarded locally;
  they do not discard other pcurve records or override the edge's serialized
  vertex identities and parameter incidences.
  Counted object-stream faces require an exact `03` or `05` terminal control;
  either form owns its first referenced carrier and remaining referenced loops.
  Counted object-stream loops require an exact edge-count lane and complete
  control tail: the loop control, three signed controls per edge, and the
  optional finite binary64/binary32 numeric extension. The first and third
  controls in each edge triple supply the source-native edge- and
  pcurve-occurrence senses; transfer requires the oriented edge occurrences to
  close cyclically and retains pcurve direction in each coedge use.
  Object-stream edges require exactly five references and one admitted terminal
  control, with no residual bytes.
  Object-stream vertex-incidence links require one roster reference and an
  exact `00` or `04` terminal control, with no residual bytes.
  Object-stream class-`18` pcurves require one complete finite line production:
  general parametric, constant-U, or constant-V. Object-stream class-`19`
  pcurves require the complete finite arc-length circle grammar. Both transfer
  over their stored parameter intervals.
  Object-stream class-`21` pcurves require the exact single-segment clamped
  Bézier grammar, complete finite suffix, and positive scalar domain.
  Object-stream class-`1a` pcurves require the exact circular diameter-period
  grammar and transfer as rational quadratic arcs over their stored intervals.
  Object-stream class-`1d` pcurves require the complete sphere great-circle
  grammar, exact redundant chart relations, and a support resolving to the
  exact class-`2a` sphere chart. They transfer as analytic spherical
  great-circle pcurves and exact model-space circles.
  Exact circular-helix constructions retain degree-1 sampled caches whose first
  and last knots are bit-identical to the analytic angular interval, so their
  edge ranges remain inside the canonical cache domain.
  Loop-to-oriented-use, oriented-use-to-incidence, physical endpoint identity,
  and body/shell binding remain unresolved.

## Evidence required to raise a score

1. Declare finite release, layout, carrier, topology, appearance, and design
   matrices for each scored envelope.
2. Manifest redistribution-cleared fixtures for every admitted matrix row,
   including malformed, unsupported, ambiguity, degeneracy, and negative
   cases.
3. Record per-fixture expected physical-byte coverage and geometry, topology,
   appearance, and design-domain losses. A gate passes only when every required
   domain through that level has no blocking loss.
4. Validate semantic fingerprints for units, placements, curve and surface
   evaluation, body ownership, orientation, trimming, feature order,
   dependencies, sketches, constraints, dimensions, expressions,
   configurations, and recomputed model identity as required by the claimed
   level.
5. Run deterministic malformed-input, resource-limit, and fuzz gates for every
   admitted parser family.

The current public scores remain L2 claimed for `standard_nested` and L1
claimed for the other recognized layouts. Capabilities above those scores are
extras until every cumulative gate through the target level passes for a
closed envelope.
