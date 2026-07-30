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
- Admitted outer `Data` declarations bind canonical or leading-underscore
  UUID-named streams to their concrete and base model-container classes and
  source ordinals. Inspection exposes these bindings on the selected outer
  stream descriptors. Alias-row ordinals resolve only through the unique object
  graph contained by the declared part stream. Feature and formula authorship
  and unresolved design accounting use that same modeling scope without
  restricting cross-graph reference targets. When the declarations do not
  select one part graph, coverage counts every retained object graph and field
  record outside the unresolved modeling scope. Other application feature
  containers remain independently retained.
- Structurally complete object graphs retain design objects, ordered fields,
  exact field classes, definition-bound values, and inter-object reference
  occurrences. Each graph retains its exact outer `Data` declaration and
  selected stream when one declared stream completely contains the graph, and
  its exact containing FINJPL segment when one completely contains the graph;
  cross-container and cross-segment references remain valid. A literal
  occupying an assigned owner slot remains distinct from both a reference owner
  and a head without an owner role. Payload references distinguish resolved
  fields, the graph's terminal null identity, and other unresolved identities.
- Equal-cardinality all-reference lists spanning every field of a design object
  retain their source-ordered columns and ordinal-aligned rows as one parallel
  reference table. Columns retain their aligned source field classes and
  resolved cells retain the exact target field class; resolved, terminal-null,
  and unresolved target identities remain distinct. Coverage partitions
  classified and unclassified columns, resolved, terminal-null, and unresolved
  target cells, classified and unclassified target cells, and matched and
  unmatched rows. A row retains its
  matching design object only when every classified column selects a distinct,
  identically classified field in that object. The match does not assert schema
  membership or semantic operand roles.
- Complete non-value entity suffixes retain escaped words and states,
  standalone `81 49` tokens, fixed `FE F6` payloads, and paged-atom state
  values as disjoint typed productions.
- Complete two-definition value chains retain the repeated schema selector,
  second role definition, catalog-resolved selected value, and structural
  ownership. Each design object retains its chain-value entity identities in
  field order. Finite and unset evaluations, atoms, controls, separators, and
  nested schema selectors are counted independently.
- Legacy typed relations retain their `body` and `param` role selectors and
  exact parameter identities when those selectors close within one identity
  run. Complete `synchrone` fields retain synchronous and asynchronous
  relation-update state independently of unresolved relation-expression
  incidence. Legacy identity runs admit and retain the `81`, `82`, `E5`, and
  `FD` record leads. Vendor-footer- and outer-directory-bounded compact schema
  programs retain their exact bytes, offsets, boundary kinds, and
  inclusive-length identifier packets. Each legacy identity run retains its
  exact outer container binding. Every complete schema role
  selector retains its fixed or paged
  production, containing identity, role representation, and value. Legacy text
  fields retain both `FE`-closed values and values carrying an inline `E3`
  paged-role tail. Field-bound roles retain literal names or exact unresolved
  schema-selector bytes, and every immediately bound role retains its field
  code. Exact `1200` name fields followed by `17C4` evaluation fields bind
  evaluated scalar, string, and integer names without resolving either role
  selector. Consecutive field-bound roles retain the intervening schema-field
  code and exact payload.
  Inclusive-length UTF-8 string packets and inline or wide signed-integer
  packets transfer as typed parameters when their descriptors resolve to the
  matching type. Their packet opcodes supply intrinsic `String` and `Integer`
  types when the exact evaluated-value name production is present and the
  containing identity has no type descriptor. A present contradictory,
  ambiguous, or unresolved descriptor prevents transfer.
  Legacy parameter transfer is restricted to the unique declared part-history
  container; unbound runs remain eligible only in declaration-free fragments.
- Complete numeric parameters and formula relations transfer when their type,
  owner, evaluation state, expression, and dependency identities resolve
  exactly. Typed unset inputs transfer independently while preventing formula
  evaluation. Each formula relation retains its stored output identity
  independently of whether that identity resolves to an entity in the same
  graph; coverage distinguishes resolved, terminal-null, and unresolved
  outputs. Every formula
  symbol occurrence retains all same-graph parameter binding candidates;
  exactly one candidate resolves the dependency. Every transferred numeric
  parameter retains its canonical
  `LENGTH`, `ANGLE`, `Real`, or `Integer` value type independently of whether
  an evaluated value exists.
  Placeholder-state, Boolean-prefixed parser-version, unprefixed
  parser-version, and opened Boolean parser-version relation-expression
  framings retain their source expressions and typed signatures. Coverage
  partitions the four framings, typed and untyped signatures, and expressions
  referenced or unreferenced by complete formula relations. Parser-version
  expressions remain native when their formula-instance incidence does not
  resolve. Exact compact lead-`12` and separator-form lead-`54` compound
  instance frames retain their program identity,
  independently resolve it within the same graph, and classify selected
  relation-expression programs without assigning unresolved input and output
  roles. Coverage partitions compact lead-`12` and separator-form lead-`54`
  framings, resolved, unresolved, relation-expression, and other program
  instances, partitions the resolved and unresolved identity
  repeated in the atom/reference slots, and counts distinct selected
  expressions. Compact-form coverage also partitions its resolved and
  unresolved `ref(h)` context identity without assigning a parameter role.
  Separator-form coverage also partitions its resolved and
  unresolved trailing entity identity.
  Parameters remain document-scoped until feature-instance ownership resolves.
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
  square roots. Typed comparisons, Boolean literals, negation, lazy
  conjunctions and disjunctions, and equal-typed lazy ternaries transfer
  predicate and conditional formulas. String literals, concatenation,
  occurrence removal, equality, replacement, case conversion, signed-integer
  formatting, Unicode-scalar length and extraction, indexed directional
  search, and finite decimal conversion evaluate with typed results. Length
  and angle values support compatible-unit decimal rounding. Length literals normalize
  `micron`, `mm`, `cm`, `m`, `km`,
  `in`, `ft`, `yard`, and `mile` to millimetres; angle literals normalize
  `rad`, `grad`, and `deg` to radians.
  Complete constraint-range productions are counted separately as dimension or
  complex-constraint ranges and finite or unset evaluations. Coverage retains
  every exact incoming reference occurrence with its source object, payload
  offset, and field or list position, and partitions ranges with zero, one, or
  multiple incoming occurrences. An incoming occurrence and
  design-field containment do not establish constraint identity, ownership,
  operands, or sketch incidence.
  Exact `Configuration` records and `configrow` successor links retain every
  stored identity and its optional same-graph resolution. Coverage partitions
  resolved and unresolved configuration references, row classes, and row
  successors. It also partitions row links that form one complete root-to-terminal
  chain from links whose order remains unresolved. These records do not count as
  transferred neutral configurations.
- Design objects consisting entirely of one exact empty principal-plane
  declaration class transfer the corresponding built-in reference-plane
  history node. Schema fields named `PRTSketch` or `Sketch` do not establish
  sketch instances.
- Several non-primary envelopes transfer connected topology or exact analytic,
  NURBS, and procedural carrier subsets. These are extras until every
  cumulative gate in one closed envelope passes.
- Every standard topology attempt reports attachment or exactly one failure
  stage. It also reports curve-support and native-endpoint-pair populations and
  partitions the final exact-pruned endpoint domains into empty, singleton, and
  multiple-choice populations with their total choice count. A mesh-quotient
  rejection reports exactly one of input structure, input cardinality,
  face-boundary cardinality, port cardinality, quotient preparation, edge-class
  constraint, or endpoint-incidence rejection. Endpoint-incidence rejection
  distinguishes absence of a complete incidence assignment from failure to
  reconstruct a boundary from complete assignments. Incidence-assignment
  rejection distinguishes input-shape, choice-pruning, fixed-assignment,
  component-domain, and component-composition failure. Coordinate-root closure
  reports distinct complete assignments as topology ambiguity rather than
  topology rejection. Topology ambiguity is partitioned into coordinate-root
  closure, endpoint resolution, and distinct reconstructed topology.
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
  surface-of-revolution construction. A profile circle offset from the
  revolution axis transfers its exact torus carrier, including spindle form.
  Face-local carriers whose complete boundary vertices select exactly one such
  torus receive that geometry. Same-carrier seams transfer as exact meridian
  arcs when their endpoints select one tube-circle center and their sweep
  equals the stored profile sweep. Missing and multiply matching intervals,
  carriers, or seam centers remain unbound atomically.
- Every consolidated line profile transfers its placed signed-distance
  analytic line carrier and stored parameter interval.
- Consolidated edge-run coverage partitions runs with zero, one, or two
  resolved support bindings and independently counts shared sampled loci and
  endpoint loci. A run with one resolved support transfers as a parametric
  surface curve; it does not imply that the unresolved partner surface has
  resolved. When its unordered endpoint loci select exactly one standard spline
  edge whose existing surface-intersection construction has no pcurves, that
  construction receives every resolved consolidated support pcurve and the
  parameter interval. A missing support binds to one free-form surface carrier
  only when the paired support pcurves lift to the same endpoint, midpoint, and
  endpoint loci and exactly one carrier satisfies those witnesses. When the
  originally resolved support carrier matches exactly one of the standard
  edge's face carriers, both consolidated pcurves bind to the corresponding
  face coedges. The partner face receives the uniquely selected free-form
  carrier, and the edge construction uses the two face surfaces directly.
  Opposite edge or coedge traversal reverses the corresponding pcurve
  parameterization. A missing or multiply matching edge, support carrier, face
  carrier, or coedge remains unbound.
- A standard circle edge without a face-side branch witness uses its exact
  two-support object-stream pcurves when both lifted endpoint and midpoint
  samples agree, match the physical endpoints, lie on the circle, and preserve
  one oriented support-carrier axis.
- Exact `a5 03 32` rolling-ball limit curves retain every correlated standard
  spline edge candidate whose two physical endpoints have unique curve
  parameters and whose endpoint and midpoint witnesses agree with every
  resolved adjacent face carrier. The solved topology binds a 3D NURBS carrier
  only when its unordered endpoint pair selects one candidate; edge reversal
  reverses the active parameter interval. These candidates seed empty endpoint
  domains but do not narrow independently established domains.
- Same-incidence standard line and spline rows with one shared complete
  bipartite endpoint relation bind equal vertex-allocation ranks to serialized
  edge-row ranks. Line and spline rows remain separate curve families;
  incomplete relations and cardinality mismatches remain unresolved.
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
  retains each face's counted allocations, ordered loop terminals, and exact
  `03` or `05` terminal control. Decode coverage counts the two controls
  independently.
  Complete `62xx` loop rosters bind to those terminals and retain alternating
  logical-member and typed-reference lanes, loop class, and absolute member
  senses. Decode coverage counts each admitted loop class and forward/reversed
  member senses independently. Each loop member binds to the unique face-local support occurrence
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
  Counted object-stream faces require and retain an exact `03` or `05` terminal
  control. Structurally complete face records require a nonempty reference lane
  and retain every ordered reference before target roles resolve; the
  reference-resolved subset assigns the first reference as carrier and remaining
  references as loops only when every target closes under that grammar. Decode
  coverage counts controls and uncounted framing independently for typed and
  reference-resolved faces, including when no topology graph closes.
  Counted object-stream loops require an exact edge-count lane and complete
  control tail: the loop control, three signed controls per edge, and the
  optional finite binary64/binary32 numeric extension. The resolved graph
  retains both framing controls, every signed edge-occurrence control, and the
  extension's four binary64 fields, odd control, and six binary32 fields.
  Decode coverage counts each framing-control pair and the extended form
  independently for structurally typed and reference-resolved loops, including
  when no topology graph closes. The first and third controls in each edge
  triple supply the source-native edge- and pcurve-occurrence senses; transfer
  requires the oriented edge occurrences to close cyclically and retains
  pcurve direction in each coedge use.
  Object-stream edges require exactly five references and one admitted terminal
  control, with no residual bytes. The resolved graph retains the support,
  ordered vertex and parameter-incidence identities, and exact terminal control;
  decode coverage counts all eight structurally typed controls independently,
  including when no topology graph closes.
  Object-stream vertex-incidence links require one roster reference and an
  exact `00` or `04` terminal control, with no residual bytes. The resolved
  graph retains the roster identity and exact control; decode coverage counts
  the two structurally typed controls independently, including when no topology
  graph closes.
  Object-stream parameter incidences retain the count-aligned curve references,
  finite stations, and compact controls independently of curve and edge
  resolution. Decode coverage counts complete incidences and their members,
  including when no topology graph closes.
  Object-stream vertex-incidence rosters retain every count-aligned parameter-
  incidence identity independently of member resolution. Decode coverage counts
  complete rosters and members, including when no topology graph closes.
  Object-stream class-`18` pcurves require one complete finite line production:
  general parametric, constant-U, or constant-V. Object-stream class-`19`
  pcurves require the complete finite arc-length circle grammar. Both transfer
  over their stored parameter intervals.
  Object-stream `b5 03 21` pcurves require the exact single-segment clamped
  Bézier grammar, complete finite suffix, and positive scalar domain.
  Object-stream `a8 03 21` pcurves require the complete degree-5 UV
  position/first-derivative/second-derivative jet, multiplicity grammar, and
  finite tail. Each knot span transfers as an exact quintic Bézier segment.
  The typed pcurve retains the positive suffix scalar. Decode coverage counts
  structurally typed and reference-resolved suffix scalars independently,
  including when no topology graph closes.
  Object-stream class-`1a` pcurves require the exact circular diameter-period
  grammar and transfer as rational quadratic arcs over their stored intervals.
  Object-stream class-`1d` pcurves require the complete sphere great-circle
  grammar, exact redundant chart relations, and a support resolving to the
  exact class-`2a` sphere chart. They transfer as analytic spherical
  great-circle pcurves and exact model-space circles.
  Class-`24` support curves and class-`14` fixed-direction curve offsets feed
  class-`2c` extrusion carriers and class-`30` surface offsets. A transferred
  class-`2c` carrier retains its directrix construction and may trim it to a
  contained active parameter interval. A class-`21` jet directrix may translate
  its parameter origin when the wrapper interval, native tail span, and
  extrusion V span agree. Class-`2c` controls `05 11` map four uniform
  class-`21` knot spans onto one V interval by an exact affine
  reparameterization. Controls `05 15` and `05 19` select and reorigin the
  terminal span of a direct class-`20` pcurve having exactly five and six
  spans respectively. Controls `01 09` and `01 15` use the class-`14` result
  interval as V bounds only when at least one class-`30` kind-`21` construction
  names the carrier and every such construction matches the source directrix
  identity, source extrusion, signed distance, plane-normal direction, and both
  result-chart bound pairs. The class-`14` source and result intervals define an
  affine parameter map. The neutral surface offset retains the defining source
  extrusion directly. Face-local class-`30`
  analytic offsets also resolve from length-closed surface frames when their
  result carrier, source carrier, kind code, and signed distance equation agree,
  without requiring the surrounding topology run to close.
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
