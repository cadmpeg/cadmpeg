# CATIA V5 `.CATPart` coverage

This document applies the cumulative support ladder to the CATIA V5 reader. It
records implementation coverage and verification gates. Byte semantics belong
in [catia.md](catia.md); unresolved byte meanings belong in
[catia-open-items.md](catia-open-items.md).

## Envelopes

The reader recognizes `V5_CFV2` part documents and classifies their geometry
storage as `standard_nested`, `fbb_only`, `zero_entity`, `e5_stream`,
`float_packed_inner_no_fbb`, `inner_no_directory`, or `unknown`.

The codec score is L1. Geometry on the `standard_nested` layout (a nested
`V5_CFV2` stream with an FBB spine and the standard edge-table delimiter)
shows as extras. `unknown` is an inspection and retention envelope and
does not inherit geometry support.

These envelopes are not closed release bands. Supported CATIA release bounds,
required and optional container segments, and the admitted carrier, topology,
appearance, and feature-family matrices have not been fixed. Scores above the
current levels require those matrices and representative fixtures.

## Cumulative gates

| Level | Required evidence                                                                                                                                             | Current result                                               | Remaining gate                                                                                                                                                                                         |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| L0    | `V5_CFV2` detection; part-kind and layout classification; bounded metadata; exact preview extraction when stored                                              | Pass in implementation                                       | Representative release-band fixtures and explicit preview-present/absent coverage                                                                                                                      |
| L1    | Nested-stream, directory, segment, extent, and record navigation; layout dispatch; external-reference and embedded-asset enumeration; named undecoded content | Pass across recognized layouts                               | Close the release/layout envelope and verify every admitted container combination                                                                                                                      |
| L2    | Placed points; analytic curves and surfaces; NURBS; correct units, parameterization, and model-space placement throughout one envelope                        | Pass for `standard_nested`; incomplete for the other layouts | Close every admitted carrier branch, persistent carrier binding, parameter chart, placement, and unit path                                                                                             |
| L3    | Connected bodies through vertices with exact ownership, orientation, trimming, placement, and transforms; structurally valid topology throughout one envelope | Incomplete                                                   | Resolve every admitted face group, endpoint registry, edge incidence, loop orientation, body/shell ownership, and cross-group membership case without topology gauges standing in for source semantics |
| L4    | Ordered feature operations with complete profiles, directions, extents, outputs, dependencies, and solved sketch geometry                                     | Incomplete                                                   | Decode construction order and dependencies, sketch membership and geometry, and complete operands for every admitted operation family                                                                  |
| L5    | Every admitted carrier and topology case; typed mainstream bodies throughout; body and face colors with source ownership and precedence                       | Incomplete                                                   | Close L2/L3 matrices, appearance bindings, and precedence, then demonstrate zero shape-domain loss across the envelope                                                                                 |
| L6    | Complete sketch constraints, dimensions, parameters, expressions, configurations, feature semantics, and re-derivable history                                 | Incomplete                                                   | Complete relation and constraint incidence, parameter values and types, feature families, configuration state, and history replay coherence                                                            |

## Implemented slices above the score

- Consolidated support tags remain separate from carrier offsets, record order,
  and header tokens. The existing circle resolver requires a unique identity
  and a compatible constant-V arc chart; a conflicting identity does not fall
  back to another geometric candidate. Duplicate-identity and conflicting-chart
  regressions exercise that rule. Complete identity-free carriers retain their
  geometry without receiving a persistent support tag.

- Outer-body FINJPL inventory and E5 candidate selection use the header-defined
  `[directory_length,directory_offset)` range. The trailing stream directory
  cannot contribute FINJPL segments, E5 census markers, E5 candidates,
  previews, version metadata, or external references.
- Stream-directory inspection retains every extent's raw trailing word in logical
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
- Exact `EB 01 R G B` visualization packets transfer as one opaque appearance
  bound to every decoded face. A singular `EC 03 R G B A` packet transfers as
  one body appearance. A population of `EC` packets transfers positionally to
  standard faces only when its cardinality and every RGBA value equal the
  governing FBB face-row population; that population supersedes an accompanying
  `EB` binding. Appearance assets are deduplicated by RGBA value. A singular
  `EC` packet accompanying an `EB` packet retains its
  color asset and remains an unresolved binding until the `FeatureForColor`
  face incidence is known. Coverage counts decoded packets, unresolved
  packets, emitted assets, and emitted bindings independently.
- Structurally complete object graphs retain design objects, ordered fields,
  exact field classes, definition-bound values, and inter-object reference
  occurrences. Each graph retains its exact outer `Data` declaration and
  selected stream when one declared stream completely contains the graph, and
  its exact containing FINJPL segment when one completely contains the graph;
  cross-container and cross-segment references remain valid. A literal
  occupying an assigned owner slot remains distinct from both a reference owner
  and a head without an owner role. Payload references distinguish resolved
  fields, the graph's terminal null identity, and other unresolved identities.
  Complete reference-signature packets retain the exact signature and its
  validated source-ordered symbol, decimal, call, comma, hexadecimal qualifier,
  and difference instruction program with payload-relative offsets. Coverage
  partitions the two prefix atoms. Fixed type atoms, the signature-length
  extent, and the `E`-symbol count equation gate the packet. The exact
  consecutive stored
  identity pair with each same-graph entity, class, terminal-null, or unresolved
  incidence state. Records sharing one pair form a source-ordered cohort whose
  members retain exact entity-record identities. A cohort retains the unique
  catalog entry selected after `_SpecList` when every member uses the
  `_SpecList` production and all present second selectors agree.
- Equal-cardinality all-reference lists spanning every field of a design object
  retain their source-ordered columns and ordinal-aligned rows as one parallel
  reference table. Each column retains its source field, list-tag payload
  offset, and source field class as one incidence. The row position retains the
  aligned list-item ordinal, and every cell retains its payload-relative byte
  offset. Resolved cells retain the exact target field class; resolved, terminal-null,
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
- Schema-selected entity suffixes retain the record-suffix-relative marker
  offsets of their outer selector and nested value selector when present.
- Direct, schema-selected, and zero-padded entity-suffix evaluations retain
  their record-suffix-relative effective opcode offsets. Named parameter values
  and constraint ranges retain the same incidence.
- Complete two-definition value chains retain the repeated schema selector,
  second role definition, catalog-resolved selected value, and structural
  ownership. Every typed definition or value role retains its selector offset,
  ordinal, selected catalog entry, and value. Each design object retains its
  chain-value entity identities in field order. Finite and unset evaluations,
  atoms, controls, separators, and nested schema selectors are counted
  independently. A chain with a non-empty first definition, a canonical
  scalar or unset second role, and an evaluation value transfers one typed
  parameter in source order. Its name comes from the first definition and its
  type from the second; exact selector, role, and evaluation-opcode offsets
  remain properties. Unsupported value states, roles, and conflicting
  candidates remain native.
- Relation-expression coverage partitions distinct expressions referenced by
  formulas, compound relation-program instances, either incidence, and neither
  incidence. An expression selected by both incidence forms is counted once in
  the referenced total.
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
  evaluation; a complete formula still transfers its typed expression and
  dependencies as an unset result when its output is unset. Each formula
  relation retains typed expression and output
  incidences with their payload-relative byte offsets, stored identities,
  same-graph entities, and entity classes; the output also retains terminal-null
  state. Coverage distinguishes resolved, terminal-null, unresolved, classified,
  and unclassified outputs and classified or unclassified expression
  incidences. Every formula symbol
  occurrence retains its UTF-8 byte offset, exact text, and all same-graph
  parameter binding candidate identities, entities, and classes; coverage
  partitions classified and unclassified candidates. Exactly one candidate
  resolves the dependency. Symbol-shaped string-literal content does not create
  a dependency. Typed signature inputs require canonical `#<digits>_` parameter
  symbols.
  Every transferred numeric
  parameter retains its canonical
  `LENGTH`, `ANGLE`, `Real`, or `Integer` value type independently of whether
  an evaluated value exists. Complete lead-`2` parameters also retain the
  second selector's exact value in `properties["catia_binding"]`, including
  an empty value, without assigning a feature or configuration role.
- Complete native numeric-pair payloads retain their two prefix atoms and two
  source-ordered nullable binary64 slots with exact bits and payload-relative
  offsets. The fixed type, layout, and value atoms and exact two-slot framing
  gate this view. A field class does not assign coordinate or parameter
  semantics to the pair.
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
  roles. Every exact reference occurrence is retained in payload order with
  its payload-relative byte offset, terminal-null state, selected same-graph
  entity, and selected class, including repeated identities. Coverage
  partitions resolved, terminal-null, unresolved, classified, and unclassified
  reference occurrences. It also
  partitions compact lead-`12` and separator-form lead-`54`
  framings, resolved, terminal-null, unresolved, relation-expression, and other
  program instances, partitions the resolved, terminal-null, and unresolved identity
  repeated in the atom/reference slots, partitions classified and unclassified
  program and repeated-identity entities, and counts distinct selected
  expressions. A program instance selecting a complete relation expression
  retains every source-symbol occurrence with its UTF-8 byte offset and exact
  text and all same-graph named parameter
  candidates; coverage partitions uniquely resolved, ambiguous, and other
  unresolved dependencies. When its typed signature and every source occurrence
  uniquely bind all distinct declared inputs, the instance retains complete
  inputs in signature order with declared types and selected entity incidences.
  Coverage partitions typed instances with complete and incomplete input
  bindings, resolved input occurrences, and distinct selected input entity
  identities.
  Each complete input whose selected entity carries a compatible finite or unset
  evaluation transfers as a typed document parameter independently of program
  output binding. Conflicting type assignments prevent transfer.
  Compact-form coverage partitions its resolved,
  terminal-null, and unresolved `ref(h)` context identity and classifies
  selected `paramout`, other named, and unclassified context entities without
  assigning a parameter role. Separator-form coverage also partitions its
  resolved, terminal-null, and unresolved trailing entity identity.
  Parameters remain document-scoped until feature-instance ownership resolves.
  Typed `Boolean` and `String` parameters transfer when their complete
  evaluation production is unset. Binary64 evaluation productions do not
  transfer as values of either type.
  Legacy named scalar packets also transfer with either a finite or unset
  evaluation when a unique acyclic descriptor chain resolves their literal
  numeric type. A uniquely bound local-input legacy relation replaces a finite
  parameter value when its typed evaluation agrees exactly, records its input
  dependencies, and supplies the expression of an unset parameter when its
  result type agrees.
  Arithmetic evaluation retains length and angle exponents through intermediate
  products, quotients, extrema, linear and cubic interpolation, trigonometric
  calls, absolute values, and square roots. Typed comparisons, Boolean literals,
  negation, lazy conjunctions and disjunctions, and equal-dimension numeric or
  equal-type nonnumeric lazy ternaries transfer predicate and conditional
  formulas. Unset-formula type checking retains unknown scalar, Boolean, and string
  values, rejects known invalid domains and non-finite constant arithmetic, and
  preserves runtime lazy-branch semantics.
  String literals, concatenation,
  occurrence removal, equality, replacement, case conversion, signed-integer
  formatting, Unicode-scalar length and extraction, indexed directional
  search, and finite decimal conversion evaluate with typed results. Length
  and angle values support compatible-unit decimal rounding. Length literals normalize
  `micron`, `mm`, `cm`, `m`, `km`,
  `in`, `ft`, `yard`, and `mile` to millimetres; angle literals normalize
  `rad`, `grad`, and `deg` to radians.
  Complete constraint-range productions are counted separately as dimension or
  complex-constraint ranges and finite or unset evaluations. Coverage retains
  complete schema-selected `Range` intervals independently of constraint roles.
  It counts no-slot productions and finite and unset interval slots separately.
  The native interval retains the exact prefix form, nullable lower and upper
  slots, scalar bits, payload-relative offsets, incoming payload references,
  and incoming object-head storage selectors. Coverage partitions payload and
  storage incidences, classified and unclassified source entities, and
  intervals with zero, one, or multiple total incoming incidences.
  Constraint-range coverage retains
  every exact incoming payload-reference occurrence with its source object,
  paired source entity identity and class when present, payload offset, and
  field or list position. Object-head storage selectors targeting the range are
  retained separately with their source object and paired source entity.
  Coverage partitions payload and storage occurrences, classified and
  unclassified source entities, and ranges with zero, one, or multiple total
  incoming occurrences. An incoming occurrence and design-field containment alone do
  not establish constraint identity, ownership, operands, or sketch incidence.
  A complete range transfers as one opaque sketch constraint only when exactly
  one total incoming incidence selects it, the source incidence resolves to the
  same graph's paired entity and object record, and that source object's
  complete owner chain reaches one transferred Sketch before another
  transferred feature. The source object record is retained as one unresolved
  native operand. A paired Range interval with a finite nominal whose bits
  equal the finite CstAttr_Dimension evaluation supplies one neutral
  millimetre length parameter on the constraint; the nominal framing, bits,
  and opcode offset remain parameter properties. The transfer assigns no
  neutral sketch entity, locus, dimensional subtype, driving state, or
  operation role; exact selector, framing, evaluation, and opcode-offset
  properties remain available on the constraint.
  An exact two-selector `Range`/`CstAttr_Dimension` production transfers a
  document PMI dimension when its admitted dimension framing carries a finite
  nominal, the paired constraint evaluation is finite, and the two scalar
  values have identical binary64 bits. The transfer carries the nullable lower
  and upper range slots as millimetre deviations and derives the PMI identity
  from the native entity identity. It assigns the selector name as an opaque
  dimension kind and no target or sketch binding; the exact selector, framing,
  evaluation, and range source remain in the native namespace. A range that
  enters the proven sketch-constraint lane is not duplicated as PMI. Range-only
  intervals and other selector sequences remain native until their semantic
  owner is established.
  Exact schema-configuration `Configuration` records retain the first
  reference's payload-relative byte offset, selected value-schema ordinal,
  entry, and name, and retain the second reference as an independently resolved,
  terminal-null, or unresolved same-graph entity identity with its
  payload-relative byte offset and selected object class when present. Exact
  `configrow` successor links retain their
  class and successor identities and the successor atom's payload-relative byte
  offset.
  Coverage partitions schema references, resolved, terminal-null, and
  unresolved entity references, classified and unclassified referenced
  entities, row classes, and row successors. Complete root-to-terminal chains
  retain ordered link incidences carrying each row identity, successor payload
  offset, successor identity, and every available selected entity class.
  Source-ordered links retain every same-graph entity in their open
  row-to-successor identity intervals. Coverage
  partitions resolved, terminal-null, and unresolved terminals, partitions
  their ordered links from links whose order remains unresolved, and counts
  source-ordered chains, intervening entities, and complete
  schema-configuration `Configuration` records among those entities. These
  records do not count as transferred neutral configurations.
- A separator-form owner declaration with the exact class `Sketch` transfers
  one sketch identity and one unresolved sketch history node. The transfer
  does not assign placement, geometry, profiles, or constraints. A `PRTSketch`
  or `Sketch` field class alone does not establish a sketch instance.
- An exact ordered owner-record reference from a transferred `Sketch` can
  retain one native sketch entity when its selected child owner group resolves
  to one child design object containing exactly one complete `2DPoint` field
  with its paired entity record. The entity keeps the source field identity and
  class as native geometry. Coordinates, placement, construction state,
  profiles, and constraints remain unresolved under DI-15 through DI-22.
- An exact source-closed relation from an admitted `2DPoint` field to a
  complete `ConstraintDYS` field transfers one unresolved native sketch
  constraint when both fields are selected by the same transferred `Sketch`
  owner list and their graph, entity, design-object, class-entry, and owner
  identities agree. The point entity, target field and entity, and every
  source/reference offset remain available. Every outbound target-field
  reference remains in source order with its identity, container, offset,
  resolution state, and resolved target identity/class when available.
  Constraint type, coordinates, parameters, driving or active state, and solver
  semantics remain unresolved; this slice does not raise the CATIA score.
- Typed scalar parameters remain document-scoped with contiguous source-order
  ordinals and unique neutral names. Changed names retain the original spelling
  in `properties["source_name"]`. Their native fields, paired entities,
  definition selectors, and values retain source identities. Structural
  ancestry does not assign a feature parameter owner or collection position.
- Native camera programs and their associated catalogs remain retained native
  records. Camera names and view/capture field-class labels do not create design
  configurations, annotation ownership, capture membership, active TPS-view
  state, or geometric targets. These relations require independent native
  incidences. Range-only size dimensions retain their source values and remain
  targetless until a target relation is established.
- Operation-like schema labels retain native design-object identity, fields,
  definition-bound values, range intervals, and relations. Structural ownership
  and class identity do not establish an operation instance or a neutral
  operation family. Parameters receive no feature owner from these labels.
- A transferred feature receives a structural `parent` only when its exact
  `owner_design_object` incidence selects another design object that also
  transfers as a feature. Self-links and owner cycles do not create neutral
  history links. A payload reference from one transferred feature's design
  object to a different transferred feature, or to a target whose complete
  owner-design-object chain reaches that feature, transfers one structural
  dependency when the target feature has an earlier ordinal. Preserve first
  relation order and deduplicate by target. Storage selectors, unresolved
  targets, incomplete or cyclic owner chains, self-links, and forward targets
  do not establish dependencies. This link does not assign an operation role.
- Design objects consisting entirely of one exact empty principal-plane
  declaration class transfer the corresponding built-in reference-plane
  history node with the design object's first-field byte offset as its source
  history ordinal. Schema fields named `PRTSketch` or `Sketch` do not establish
  sketch instances.
- Exact separator-form owner declarations for `GSMPlaneAngle` and
  `GSMPlaneOffset` transfer unresolved constructed-reference-plane family nodes
  with source order, identity, and structural parent linkage. The transfer does
  not assign support, angle, signed offset, normal, in-plane frame, or
  construction dependencies; this slice does not raise the CATIA score.
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
  closure, endpoint resolution, and distinct reconstructed topology. Bounded
  search exhaustion is partitioned into quotient preparation, incidence
  enumeration, and endpoint resolution.
- Same-carrier standard cone line rows retain only endpoint pairs whose segment
  is a straight generator through the analytically derived cone apex. The
  existing endpoint surface-membership and trim-incidence checks remain
  independent gates.
- FBB-only groups with one exact native endpoint pair for every edge row can
  close topology directly from their two counted edge tables and counted vertex
  table. This path reuses the existing face-incidence and edge-class quotient,
  does not infer endpoint identities from trim order, and falls through to the
  bounded mesh solver when the direct closure is not complete.
- The standard face spine admits both unflagged `30 04 04 ff` and flagged
  `b0 04 04 ff` rows. Standard edge-table handle width is selected uniquely
  from one, two, or three bytes by landing on the complete counted vertex
  table. A two-handle row is a complete boundary segment; longer rows retain
  flanking endpoint ports around their interior trim handles.
- E5 plane carriers solve their in-plane frame from endpoint UV values and
  referenced vertex positions. A full-rank endpoint set uses the unique
  orthonormal least-squares frame within `2e-3` mm. A rank-one diameter set
  uses the known normal supplied by the adjacent analytic carrier to complete
  the perpendicular axis; the simultaneous sign gauge requires the first
  nonzero component of the `u_axis` to be positive. A missing or ambiguous
  normal, non-finite endpoint, non-orthonormal fit, or residual above the
  tolerance leaves the plane carrier unresolved.
- Rooted E5 loop traversal uses the complete loop relation and the face-aligned
  root sign at roster position plus one. Every member must yield the same
  nonzero loop-local sign. The admitted root endpoints are `H=+1, S=−1`;
  other endpoint signs abstain. Shared-edge parity checks the resulting
  traversal; a missing body root or conflicting relation abstains.
- Analytic E5 face orientation composes the face and root relations. Plane
  faces use the signed area of their oriented line/circle boundaries in the
  fitted chart. Outer and inner loops must give the same face sense; an
  unsupported boundary carrier, zero area, or disagreement abstains.
- E5 edges require five object references followed by exactly one allocation
  terminal. The admitted terminal values are `01`, `02`, `21`, `22`, `25`,
  `29`, and `2a`; value and addressing form remain separate from endpoint
  direction. The five object references also admit the shifted-u16 `30`
  form. Unsupported terminals or incomplete references reject the topology
  graph. Original bytes remain in native records.
- E5 jet and NURBS bounds resolve their one-based distinct span code and
  local scalar to the native knot parameter before selecting occurrence
  direction. NURBS knot repetitions do not count as extra arcs; a nonzero
  first knot remains part of the conversion even for code one. Arc scans
  charge the work budget.
  Unknown arc numbers, missing representations, repeated entries, and
  out-of-span values leave the binding unresolved. Paired analytic line/plane-jet supports use
  the line's full affine UV map and owning carrier chart for the physical
  circle branch; negative slopes and nonzero offsets remain intact.
- E5 curve-support sides admit direct p-curves and finite acyclic chains of
  intersection-support wrappers. Missing sides and wrapper cycles reject the
  graph. When a wrapper side does not resolve directly, the two radial loop
  occurrences supply the ordered support sides only after endpoint and
  parameter-range agreement.
- Zero-entity physical edges, oriented uses, and vertices bind through the
  complete source allocation graph. Endpoint coordinates validate the bound
  vertex owners: every contributing coordinate must agree pairwise within
  `2e-3` mm. Geometric proximity does not establish source identity.
- Compact and object-id class-`3b` records retain typed two-support chamfer
  references, result kind, contact order, support halfspaces, and either two
  setbacks or a setback and acute angle. The exact referenced result carrier
  supplies geometry. Compact contact ownership requires the adjacent result
  kind and two parallel ordered bounds; a further pair must close their
  rectangle. Unrelated following records remain independent.
  Consolidated parameter-space points retain source-local compact operands
  with exact inline or one- through four-byte integer encodings and ordered
  scalar payloads. Controls enforce scalar count, negative centered-domain
  start, and explicit unit direction. Owner charts require complete
  constant-axis tuples or the implicit-line half-span, midpoint, and boundary
  relation.
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
- Complete consolidated class-`0x27` carriers retain all selector-specific
  scalar layouts in the native namespace. Direction-bearing `e4` and `c4`
  carriers transfer a plane frame and bind pcurve sides only through unique
  endpoint lifts to object-stream vertices. Directionless `ec` carriers remain
  native-retained until their axis and tail roles resolve.
- Complete `b2 03 16` rational clamped curves, `a5 13 16` degree-five clamped
  curves, and `b2 03 0f` bounded spatial circles transfer as standalone wire
  edges when they are the complete geometric population and no surface graph
  is present. Their exact knot or angular domains supply evaluated endpoint
  vertices and wire-body ownership.
- Consolidated edge-run coverage partitions runs with zero, one, or two
  resolved support bindings and independently counts shared sampled loci and
  endpoint loci. A validated consolidated sphere binds one pcurve side when
  its stored azimuth and latitude lift both endpoints to stored vertices and
  exactly one sphere satisfies that relation. A validated consolidated torus
  applies its major and minor chart scales before the same unique endpoint-lift
  test. Multiple matching carriers leave the side unresolved. A run with one
  resolved support transfers as a parametric
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
  An explicit class-`0x20` support identity that equals one unique standard
  `00 33` carrier tag uses that carrier's stored chart directly. Its pcurve
  endpoint lifts must select the same serialized edge endpoints before the
  edge or face-side binding is admitted; duplicate tags, unknown carriers,
  and failed lifts remain unbound.
  An explicit support identity may first pass through the unique outer alias
  closure: a direct alias preserves its tag, and a grouped alias selects the
  unique direct target in the same group. Duplicate raw alias tags, missing or
  multiply targeted groups, and failed canonicalization do not fall back to
  the raw identity. This closure does not rewrite B5 object-stream or
  allocation namespaces.
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
- Same-incidence standard line and spline rows retain a shared complete
  bipartite endpoint relation until exact trim-cycle and endpoint-port
  constraints bind the physical rows. Allocation-rank reduction applies only
  to an otherwise equivalent edge-identity gauge; line and spline rows remain
  separate curve families, and incomplete relations and cardinality mismatches
  remain unresolved.
- Repeated standard edge-row handles that match more than two face occurrences
  retain a deferred face domain. Native edge ownership and complete endpoint-
  closed trim assignment must select the repeated incidence before topology
  transfer.
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
  patch from available pcurves. Its neutral revolution construction maps the
  stored angular surface-parameter interval affinely to the physical radian
  interval, so the construction and cache share the native chart.
  Consolidated and object-stream tori retain independent active major- and
  minor-angle intervals and their centered full-turn chart domains.
  Object-stream tori additionally require the complete lead, right-handed
  orthonormal frame, and zero tail; malformed frame, tail, or range/domain
  relations reject the complete record.
  Consolidated and object-stream cones retain their active azimuth interval,
  centered full-turn domain and slant interval. Consolidated cones require
  their fixed chart-tail scalars. Object-stream cones require the complete
  lead, signed orthonormal frame and unit parameter coefficient. Their finite
  V shift maps the external chart to local generator length. Neutral geometry
  includes the reference radius and radial-plane generator angle; pcurve and
  isoparametric circle transfer preserve the stored frame handedness.
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
  logical-member and backward edge-distance lanes, loop class, and absolute member
  senses. Decode coverage counts each admitted loop class and forward/reversed
  member senses independently. Each loop member binds to the unique face-local support occurrence
  whose slot equals the loop terminal minus that member; the complete
  face-local binding is atomic and retained as support-record ordinals. A loop
  retains its complete sense-oriented model endpoint tape when every support
  lifts directly or exactly one missing pair is bounded by lifted neighbors
  and every cyclic join closes within the format tolerance. Each odd lane selects a
  physical-edge record by backward distance from the loop ordinal. The complete
  allocation graph joins that edge to its `2569` header, ordered `0638` uses,
  face-local supports, `05xx` incidences, and persistent `5d06` vertex owners.
  Native allocation edges retain all six record classes through exact identities.
  Coverage counts complete allocation edges and bound loop-to-edge distances.
  The decoder atomically lowers a complete graph whose endpoint contributions
  agree into neutral faces, loops, coedges, edges, vertices, points, and pcurves.
  Physical edges, side-use coedges, and vertices use their source record ordinals.
  Geometry validates the bindings and supplies derived carriers and coordinates.
  Each alternate face occurrence keeps its oriented use curve and coedge sense.
  A complete terminal ownership root supplies body and shell identities. Without
  that root, the graph uses deterministic derived body, region, and shell identities. Any incomplete, ambiguous, or inadmissible
  relation candidate retains the atomic wire fallback. A direct carrier that
  cannot be evaluated or oriented remains a reachable unknown carrier; the
  topology candidate commits only when the complete graph remains admissible.
  A closed loop transfers as an independent wire body when every member has a
  transferred model-space curve, every member has a lifted model-space endpoint
  pair, and the oriented endpoint tape closes within the format tolerance. The
  wire contains one edge per face-local support occurrence and one vertex at
  each cyclic join. A complete model-carrier parameter pair transfers to the
  edge parameter range in loop direction; an opposite packed sense reverses
  that range through an oriented direct carrier or a reversible helix
  construction. Other procedural carriers without a reversible construction,
  and incomplete or non-finite pairs, leave the range absent without discarding
  the edge. This transfer does not assign source physical-edge
  identity, radial use, endpoint-incidence identity, or source body and shell
  ownership when no complete terminal ownership root covers the transferred
  loops; those native bindings remain unresolved.
  When a complete terminal ownership root covers the aligned support-run roster,
  every transferred closed loop shares one derived wire body, region, and shell
  in the root's face-roster order. This binds the neutral container to the exact
  source body and shell hierarchy without assigning physical-edge identity,
  radial use, endpoint-incidence identity, or face topology.
  In the object stream, full-pcurve endpoint loci are geometric fallback
  candidates only. Conflicting candidates for one edge are discarded locally;
  they do not discard other pcurve records or override the edge's serialized
  vertex identities and parameter incidences.
  Object-stream decoding reconstructs each directory descriptor independently
  and partitions its A8/B5 records at each byte that is neither a frame nor a
  complete owned allocation. Object identities, topology roots, and
  standard-surface evidence do not merge across those run boundaries. A
  topology run can admit one uniquely referenced isolated geometry frame from
  its own logical stream. Decode coverage reports decoded, selected, and
  unselected object-stream run counts.
  Counted object-stream faces require and retain an exact `03` or `05` terminal
  control. Structurally complete face records require a nonempty reference lane
  and retain every ordered reference before target roles resolve; the
  reference-resolved subset assigns the first reference as carrier and remaining
  references as loops only when every target closes under that grammar. Decode
  coverage counts controls and uncounted framing independently for typed and
  reference-resolved faces, including when no topology graph closes. A loop
  named by more than one resolved face invalidates those face bindings; the
  affected faces remain outside the connected graph while independent
  face-loop components may transfer.
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
  A transferred face with one closed loop derives `outer`. A planar face with
  multiple loops derives one `outer` and `inner` for the remaining loops only
  when every occurrence has a finite line pcurve whose lifted endpoints agree
  with the serialized edge endpoints, one unique largest polygon strictly
  contains every other polygon, and no boundary is degenerate or touching.
  Curved, non-planar, disjoint, overlapping, nested-hole, and ambiguous
  arrangements retain `unspecified` boundary roles.
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
  great-circle pcurves and exact model-space circles. Their edge class-`06`
  stations bind loop endpoints and vertex-incidence coordinates; chart-bound
  stations are the endpoint fallback when no valid edge span exists.
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
  result and source carriers have the same family, kind code, and distance
  magnitude required for planes, cylinders, spheres, and tori,
  without requiring the surrounding topology run to close. Cone kind `11`
  requires matching oriented frames and a signed native-normal distance, with
  the generator translation matching the source/result V-shift difference.
  Its explicit result cone supplies the neutral geometry and translated chart;
  the complete native offset construction remains retained.
  Exact circular-helix constructions retain degree-1 sampled caches whose first
  and last knots are bit-identical to the analytic angular interval, so their
  edge ranges remain inside the canonical cache domain.
  Each transferred standard-nested loop retains an ordered vertex use for every
  boundary edge occurrence. The vertex use points to the boundary traversal
  endpoint and anchors after the corresponding coedge; the endpoint identity is
  the same topology vertex identity used by the emitted edge.
  Each transferred E5 loop retains an ordered vertex use for every oriented
  edge occurrence. The loop solver selects the traversed endpoint from the
  edge's stored vertex identity and member sense, and the use anchors after the
  corresponding coedge.
  Each transferred object-stream loop retains an ordered vertex use for every
  oriented edge occurrence. The vertex use selects the traversed endpoint from
  the solved edge sense and anchors it after the corresponding coedge; the
  endpoint identity is the same topology vertex identity used by the edge.
  Loop-to-oriented-use and oriented-use-to-incidence source relations remain
  unresolved.

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
   configurations, and recomputed model identity as required by the scored
   level.
5. Run deterministic malformed-input, resource-limit, and fuzz gates for every
   admitted parser family.

The current public score is L1. Capabilities above L1, including
standard-nested geometry, are extras until every cumulative gate through
the target level passes for every admitted layout.

Storage-property members in bounded type-`0x8e` FINJPL segments are retained in
`storage_property_members`, separately from `alias_rows`. The native records
preserve direct or nullable state, complete four-byte allocation slots, framed
numbered labels, raw tags, unassigned bytes, and containing segment identities.
Their labels and slot low bytes do not assign history records or face ordinals.

Strict childless inline records retain self, anchor-field, and optional
value-field incidences in `inline_fields`. Each incidence carries its exact
body offset and same-graph entity resolution. The complete wide schema chain
joins the associated catalog entries for `Projected3DGeometrySet`,
`CATEarlySpecBrpLocation`, and `data`. Compact schema tokens remain unnamed.
Opaque childless bodies retain their complete bytes without these roles.

Zero-entity face terminals assign positive (`05`) or negative (`03`)
face-to-surface orientation. Complete loop binding requires the matching outer
class (`41` or `c1`); contradictory orientation retains the native face without
binding its loop roster.

E5 NURBS current-limit programs admit the identity parameterization with disabled extrapolation. AA pcurves use the two tail-defined arc-local limits, including an inset lower limit. E7 surfaces require a current rectangle equal to the complete knot domain. Nonidentity parameterizations, active extrapolation, malformed limits and unadmitted surface subdomains retain native bytes and abstain from this geometry route.

E5 curve supports retain complete mode-0, affine mode-3 and sampled mode-5 programs in `e5_curve_supports`, indexed by physical source offset and persistent record identity. The graph and Native arena share one parser. Unknown modes and malformed tails remain in the original bounded source payload. Accuracy, fit tolerance, fixed limits, active limits and both sampled lanes remain distinct. The reference facet supplies an exact lifted curve or its parametric surface-curve construction. A two-surface neutral context requires normalized line pcurves with independently affine lifts to the same ordered locus; endpoint agreement and sampled correspondence sites do not establish that map. The source co-facet and complete correspondence remain in Native data.

Indexed visualization coordinates bind width-three trim endpoints to counted coordinate rows by exact binary32 coordinate equality. Multiple trim handles can bind one logical vertex. Complete admission requires every coordinate row to be selected; coordinate rank does not bind endpoints.

FBB/E5 route precedence follows complete standard and two-table FBB-only walks before the positive-vertex partial-spine fallback. Vertexless spines remain unresolved even with coherent E5 elsewhere. Subordinate width-one/D(1) table pairs and width-two kind-2-only populations retain their nested source bytes; E5 topology comes from the independently bounded E5 graph. Geometry, topology and Native support inventories share the E5 frame-span reader. At an exact frame end, complete interstitial `13`/`83` payloads remain outside geometry inventory and route-witness searches.

Object-stream B5/A8 loops admit the complete 62-byte numeric terminal with six distinct canonical compact controls, four finite binary64 fields and six finite binary32 fields. Terminal admission exhausts the owning frame and keeps member identities, counts and signed occurrence controls independent. Compact construction-domain owners, A9 loops, E5 loops and FBB trim cycles use their own bounded productions.

Object-loop member triples retain the edge occurrence sign, the composite pcurve-to-edge-curve parameter orientation and the pcurve occurrence sign separately. Class-`06` stations retain both the local parameter and compact arc number. Endpoint station selection accepts equal repetitions and rejects conflicting arc numbers even when the local scalars match.

Numeric object-loop terminals retain patch-aware U/V rectangle corners and paired Cartesian bounds. Admission requires nondecreasing patch identifiers, nondecreasing local scalars within one patch, a terminal compact zero, and finite ordered Cartesian low/high pairs. Local resets across patch boundaries remain admitted; stored boxes are retained without regeneration.

A8 freeform surfaces consume their pole and weight grids inside the declared logical frame after descriptor reconstruction. Nonphysical extent order preserves the complete neutral model. Unframed pcurve gaps do not supply pole grids or bridge object-stream namespaces. The fixed terminal-span tail retains local intervals and identity affine pairs independently of the full knot domain.

Vertex-incidence links accept exactly terminal `00` or `04` after one complete object reference and retain the terminal separately from the resolved roster. The two controls use the same incidence resolution. Strict frame closure rejects missing and extra terminal bytes. Class-`5d` dispatch uses the complete payload signature.

## Float-packed native identity fixtures

The frozen plane and cone fixtures contain 18 and 23 complete B5 frames,
respectively, inside nested containers with a one-byte `SurfacicReps` and no
FBB spine. Their vertex rosters contain one vertex-local incidence each;
every adjoining curve station agrees at the vertex. The plane main stream
has 762 bytes and its container has 1,026 bytes. The cone main stream has
1,026 bytes and its container has 1,290 bytes.

End-to-end tests require one body, region, shell, face, loop and surface;
three plane or four cone edges, coedges, vertices and pcurves; complete typed
edge, vertex, roster and parameter-incidence coverage; and zero unresolved
faces or loops. The cone uses a nonzero reference radius and a generator
angle different from π/4. Point coordinates, neutral validation, and every
geometry annotation, including procedural curves and surfaces, are checked.
The sole decode loss is the `topology.b5-gauge-substituted` warning.

The smaller nine-frame coordinate-backed triangle remains a routing fixture.
Its empty edge payloads do not exercise native edge or vertex identity grammar.
Successful fallback transfer does not establish incidence-roster coincidence.

## Native object-loop boundary roles

B5 face transfer uses the second loop control to select the unique outer loop
and each inner loop. Classification applies to analytic, freeform and opaque
carriers and does not require planar pcurves or a containment calculation.
Absent or multiple outer selections leave the loop roles unspecified.
The first loop control retains face-to-carrier orientation independently;
shell orientation and neutral chart conversion remain separate relations.

The frozen plane-with-hole graph has 34 frames and two three-edge loops.
Its face lists the inner loop first, and its boundaries are degree-one NURBS
pcurves. Transfer selects loop 400 as outer and loop 401 as inner from their
location controls. All six vertex loci and every source annotation are checked.
The plane and cone identity fixtures also check the middle occurrence sign
against their common curve/pcurve stations; each direction product is `+1`.

## Complete object-stream body populations

B5 face-body transfer requires the complete selected source population.
It retains an incomplete graph as native data and publishes no partial face
body. The neutral body-kind gauge uses complete physical-edge incidence: closed
components give solid, open components give sheet, and mixed or non-manifold
incidence gives general. This gauge does not establish the source cell and
domain dimensions. Ownership counts and terminal controls do not assign body
kind. Complete closed-loop wire recovery remains a separate transfer.

The completeness regression starts with two faces sharing all three edges.
It removes one face's pcurve binding and marks the source graph incomplete.
Transfer declines without mutating the neutral document; dropping that face
would produce a sheet from a topology-closed population.

## Single-carrier object-stream faces

A counted face has one carrier and any number of owned loops. Transfer rejects
an extra surface alias in a loop slot. Carrier construction dependencies remain
separate from face ownership.

The 79-file byte check resolves all 32,870 later reference slots of 30,056
complete counted face records as loops repeating the face carrier. Arduino's
unique 54-loop face matches the sibling STEP file's unique 54-bound face by
plane origin and axes; both faces have one carrier.

## A5 rolling-ball header values

The parser retains every canonical width-1 header value and applies one
degree-5 jet grammar. The regression covers all 64 canonical values with
unchanged site data and rejects all 192 noncanonical bytes.

An independent byte check joins all 183 typed A5 jets in clothes-basket,
spray-bottle, NIST CTC-02, and NIST CTC-05 to unique native frames. Every knot
and all 30 value and derivative channels per site match exactly. The nine
header bytes are `05`, `09`, `0d`, `19`, `1d`, `25`, `2d`, `35`, and `3d`.
Source-extent reconstruction closes the spray-bottle frame at offset 809,757.
All sites satisfy equal ball radii and the opening-angle equation within
`1e-9`; the largest relative radius residual is `1.40e-12`, and the largest
angle residual is `7.72e-12` radians.

## A5 rolling-ball tail admission

Admission requires a complete tail after the three geometric jet blocks.
The parser checks exact branch lengths, finite lanes, current and maximal
limit containment, both reference-scalar copies, controls, and map terminals
before it retains site data. Arbitrary frame-bounded continuation bytes no
longer qualify as a complete rolling-ball carrier.

The byte check resolves 183 source tails: 170 absent scalar jets and 13 explicit
scalar jets. All 5,265 quintic samples stay inside their endpoint value range
and retain the stored decreasing direction. Source parameter domains and six
records with exact source knots support the parameter interpretation. All four
NIST maps fit all four candidate source-curve domains, so no target is assigned
from that relation. Raw tail controls retain their source polarity.

The frozen `standard_a5_rolling_ball` input lacks a tail and remains a rejection
witness. `standard_a5_rolling_ball_complete_tail` adds the complete 79-byte
production. CATIA has 23 frozen inputs and 69 golden files. The golden-file floor is 69.

## Consolidated edge header integers

The existing parser checks canonical compact header widths before it reads the
five-reference edge payload. The header-value regression covers width
transitions and the native values 2,232 and 62,701, with every other node field
unchanged.

Independent byte joins verify 32,712 typed edge frames in 33 of the 77
CLI-checked CATParts. Their 53 logical sources use 105 header values from 1
through 62,701, with widths 1/2/3 in 32,606/84/22 records. All five reference
forms and the complete terminal match the typed native data. The first
31,420-record population has repeated header values in every source and
decreasing values in physical-offset order in 29 sources. The eyebolt and
spring four-edge cycles both use `[2,1,1,1]`, with degree two at every endpoint.

## Gap-search temporary storage

Saved placement copies belong to one recursive gap-search step. The solver
admits them against the live temporary-byte budget and releases that admission
after restoring the placement sequence. The regression checks temporary-byte
refusal, exact assignment equality, and 32 repeated searches under one limit.

This correction lets nist_ftc_10_asme1_ct5315_rb decode under its unchanged
2,207,176,216-byte retained ceiling. Its previous attempt exhausted that
ceiling; 396,932,880 bytes of cumulative charges were saved-placement copies.
The corrected model has 310 faces, 923 edges, and 3,599 native entity records.
The CLI check reports zero errors and zero warnings. Its 1,292 consolidated
edges complete the header census above. This is a structural validation result;
the decode's fidelity report retains its declared losses.

## Four-reference consolidated edge frames

An independent scan of all 79 CATParts finds 131 complete four-reference
class-`5e` frames: 90 in bike-handle and 41 in snap-ring. Every frame has one
descriptor owner and closes inside its reconstructed logical source. Exactly
130 frames are immediately followed by another complete width-coded frame.
Terminals `01/02/21/25` occur `32/3/93/3` times. All four reference lanes use
the one-byte backward-distance or owned-child forms. These are source-byte
grammar checks; the four operand roles remain unassigned, and the existing
five-reference parser abstains on this production.

## Object-stream edge terminals

The existing B5 parser accepts all eight observed terminal forms and requires
payload exhaustion. Its regression checks that changing only the terminal
preserves the edge identity, curve reference, ordered vertices, and ordered
parameter incidences.

An independent complete-payload screen over all 79 CATParts finds 130,884 B5
edge layouts. The counts for `01/02/21/22/25/26/29/2a` are
`4,921/455/114,559/7,454/998/1/1,465/1,031`. This is a structural byte census,
not a count of transferred neutral edges. All 30 groups with identical ordered
reference tuples and different terminals use `21` and `29`. Each frame in
those groups, and the single `26` frame at physical offset 840,956 in
ssd-crucial-mx500-carter-inf, has unique descriptor ownership and exact logical
source closure. The rare frame selects edge object 4427636 and references
`(4427641,4427634,4427627,4427642,4427643)`.
Face 4427632 names loop 4427633, whose first edge is this `26` edge. The
loop's four signed edge occurrences close through exact vertex identities;
the other terminals are `21`, `22`, and `21`.

These checks establish the wire grammar and retained identity separation.
They do not establish the terminal's meaning or a rule for choosing a new
terminal from geometry.

Class-`0x18` control verification covers 4,507 parameter-space records in 31
files: 3,634 low-control scalar lanes and 102 explicit unit directions.
All records were joined to exact native frame bytes. The 45 owner charts in
14 files satisfy the complete side controls and scalar relations, including
50 implicit-axis side records. Regression tests reject contradictory controls,
non-unit explicit directions, and each incorrect implicit-line scalar.

Class-`0x23` affine-map verification covers 454 eight-scalar definitions in 21
files. All definition frames match their native bytes. Every stored interval is
increasing, both tolerance words agree and are positive, scale is positive, and
both endpoint residuals satisfy the stored tolerance. The maximum ratio of
residual to tolerance is 0.7071067811865475. The 166 complete analytic-circle
runs in 10 files close in one logical source and have exact target-circle range
agreement. Ninety source descriptors repeat the symmetric map interval,
including 12 explicit unit directions. The 108 reversed-slot definitions are
identity maps; reversed non-identity direction remains open.

One identical map payload accompanies two center pairs separated by 44.45 mm.
Both center pairs and radii match the sibling FTC-08 STEP geometry within
`1e-8` mm. Multiple STEP Z placements share each pair and radius; this comparison
checks the pair and radius. Circle geometry and placement remain supplied by
the bound carrier.

Class-`0x24` verification covers all 79 physical CATIA files. The independent
descriptor-scoped byte inventory contains 7,141 complete scalar relations in
31 files. All 13 selector forms consume the complete finite scalar lane and
satisfy the interval, map, site, and accuracy rules. There are 3,751 explicit
accuracy closures, 3,390 elided closures, eight negative signed accuracies,
4,028 positive affine scales, 48 negative affine scales, and no zero affine
scales. The maximum identity endpoint difference is
`1.5273690223693848e-7`; the maximum affine residual is
`1.862645149230957e-9`.

Every scalar frame was also decoded through the production codec in a
constructed standard CATIA container. All 7,141 native payloads and typed
relations match the independent byte parse exactly, including signed zero.
The neutral geometry arenas remain empty. The new
`consolidated_domain_relations` arena retains the complete frame, bounded
source index, logical source offset, and typed source relation, sites, and
accuracy closure. Unresolved operands do not create geometry.

All 5,276 compact definitions owned by native edge nodes in 33 files match
independent four-frame source proofs. Their use vectors and senses agree with
the compact operand. The 4,901 following definitions use node selectors
`(1,2)`; the 375 preceding definitions use `(2,1)`. Regression tests cover
both native round trips and reject contradictory operands and senses.

The scalar golden fixture covers negative affine scale and signed repeated
accuracy, plus selector `0001` with both target sites and an elided
negative-zero accuracy. The 23 earlier frozen fixture inputs are unchanged;
their decode snapshots only add the empty domain-relation arena.

Fresh whole-file decoding and checking covers 35 files with 4,362 scalar
relations and all 5,276 owned compact definitions. Their complete decoded
documents match the earlier models outside the intended native parameter
projections and the new arena. Apartman reaches the existing 512-step ordered
face-constraint limit; its 2,779 scalar frames are covered by the separate
frame-grammar verification above.


Class-`0x25` verification scans all 79 physical CATIA files through
`cadmpeg inspect`. It finds 6,091 distinct length-closed B2/B3/B4 flag-`03`
candidates: 6,077 complete two-operand scalar maps in 29 files and 14 candidates
outside that grammar. The complete maps comprise 4,604 affine relations,
27 target-only relations, and 1,446 sampled relations. All nine selectors
consume their complete frames and satisfy the numeric invariants.

Among sampled maps, 1,409 source arrays increase and 37 decrease. All samples
remain within their named intervals. Sample extrema differ from the interval
bounds bit-for-bit in 276 records; seven differences exceed the stored
accuracy. The arrays therefore retain their pairing and do not supply interval
endpoints. Two spray-bottle affine maps retain fit tolerance `0.001` separately
from signed accuracies `1.584409889836592e-8` and `1.1371036519709368e-8`.

Every complete map was also decoded through the production codec in a
constructed standard CATIA container. All 6,077 native payloads and typed maps
match the independent byte parse exactly. Checking the resulting document
reports zero errors and warnings; its neutral geometry arenas remain empty.
The domain-relation arena retains allocation addressing forms, sample block
order, source positions, signed accuracy, fit tolerance, and target sites.
Operand binding and the separate multi-operand productions remain open.

Fresh whole-file comparison covers all 79 CATIA files: 78 decode and pass
neutral and CATIA native validation, retaining 6,015 complete class-25 maps.
Class-24 data and source positions remain exact. The complete documents match
before and after except for the independently verified class-25 native
projections and domain-relation identities. No whole-file edge definition or
class-25 descriptor binds these maps. Arduino validation requires a verification
budget of one trillion work units and 256 million collection items; the default
CLI work ceiling is exceeded. Apartman reaches the same existing 512-operation
face-constraint refusal in both versions. Its 62 complete maps are verified in
the separate frame-grammar container above.

The parameter-map golden fixture covers paired arrays, decreasing source
orientation, interior target sites, a distinct fit tolerance, and elided
accuracy. All 72 earlier golden files, including their frozen inputs, remain
byte-identical. Regression tests cover every bounded sample count from 2
through 12 and reject a count exceeding the B-family payload capacity,
contradictory map data, nonmonotone arrays, wrong frame flags, and A8 frames.


A8 class-`0x25` verification scans all 79 physical CATIA files through
`cadmpeg inspect`. Of 1,733 signatures, 1,728 complete `82` sampled maps occur
in nine files. Four length-closed oil-tank records begin `84`; an Apartman
signature declares a frame extending beyond the file. The complete maps have
12 through 232 pairs: 1,026 co-curve arrays decrease and 702 increase. All
reference arrays increase. Thirty-seven maps have zero fit tolerance, and 66
have active reference intervals strictly inside their leading domains.
Sample extrema differ bit-for-bit from domain endpoints in 1,360 maps; nine
endpoint differences exceed fit tolerance plus roundoff. Interval endpoints
therefore remain distinct from sample extrema.

All 1,728 maps and their 3,445 distinct file-local operand identities have
unique physical frames in their descriptor sources. Twenty wrapper operands dereference once
and close in the same source. The first operand is a wrapper in 19 maps and
the second in one; direct operands include lines, circles, sphere pcurves, and
UV jets. Operand-role verification fully parses lines and wrappers; other
families are identified by their complete frame class and support prefix.

Every complete map also passes the production codec in a constructed standard
container. Its payload, object identity, paired arrays, domains, fit tolerance,
and active interval match the independent byte parse exactly. The resulting
document passes checking with no errors or warnings and has no neutral
geometry. The native `sampled_parameter_maps` arena retains exact source
positions and payloads. Its wire loader rejects data contradicting the payload.
The golden fixture adds zero fit, a 232-pair positional count, a proper active
subrange, and the Arduino witness. Earlier decode snapshots change only by
adding the empty arena; all earlier fixture bytes remain exact.

Arduino object 756464 at byte offset 19,668,336 pairs direct line pcurve 756466
with wrapper 756467 and its quadratic pcurve 756468. The two cylinder supports
have identical geometry and distinct angular chart domains. All 17 parameter
pairs lift to the same model-space points; the maximum separation is
`6.280369834735101e-16` mm, below the binary64 roundoff bound
`4.97379900441478e-14` mm. These supports do not establish a unique intersection
construction. An admitted extrusion uses the first representation's exact
lift over the active reference range. Both operands must resolve before this
admission. Offset sources use that active range, including a strict subrange
of the underlying pcurve domain. Between-sample interpolation remains open.


Six sampled-map extrusion directrices in `v6-engine-air.CATPart` and five in
`v6-engine-filter-assembly.CATPart` use parameterized first-pcurve lifts. Each
lift passes 33 independent tensor-basis evaluations over its active reference
interval, for 363 evaluations. The maximum model-space separation is
`6.112319729142702e-14` mm. Surface isocurves map their knots to
the pcurve parameter, including decreasing surface-coordinate maps. Cylinder
lifts preserve the pcurve's origin and scale. The admission path supports plane
lifts, affine NURBS isocurves, and constant-angular cylinder pcurves; other
charts remain unadmitted. Sampled maps do not inherit class-24 jet-origin or
four-span chart translations.

The eleven old intersection procedures are removed. Their first support
carriers retain exact geometry. Second-operand planes 323806 in the air file
and 356098 in the filter assembly have no remaining model use and are omitted;
later surface identities and their references change accordingly. Independent
comparison verifies all other model arenas and all retained source arenas
exactly.

Whole-file verification covers all 79 CATPart files and checks 78 decoded
models. Their 1,728 sampled maps match independently inspected payloads,
typed values, object identities, and physical and logical source positions.
Only the two files above change model geometry. `apartman.CATPart` retains
the baseline refusal at the 512-face ordered-constraint work limit; its 62
complete maps are verified separately in the bounded production fixture.

Standard-decoder midpoint arithmetic retains finite results for finite
endpoints whose sum overflows. Behavioral regressions bind a quintic limit
curve on positive and negative domains of magnitude `1e308` and retain both
antipodal circle seams at an origin coordinate of `1e308` mm.

The CTC05 standard spline row at physical offset 1,316,188 closes in nine
bytes before the next row. Its associated native run closes in 742 bytes:
two class-20 UV pcurves, one class-25 affine parameter map, two class-06
incidences, and one class-5e edge. The map's nine binary64 values exhaust its
payload and contain no Cartesian pole or knot lane. Edge 174 retains unknown
curve 343064 with source identity `cgm-edge-support:083306`. The checked model contains 403 standard curves, and Native retains 68
consolidated pcurves.

| Standard curve kind | Records |
| --- | ---: |
| Line | 181 |
| Circle | 188 |
| Ellipse | 8 |
| NURBS | 8 |
| Unknown | 18 |

The sibling STEP file supplies one endpoint match among 514 edges at a
`2e-3` mm gate, with maximum endpoint residual `6.105607469691817e-6` mm.
Its cubic eight-pole curve is an export oracle. None of its eight binary64
XYZ triples occurs in the CATPart. Only its first and last binary32 triples
occur, once each, both inside `05 08 01` vertex rows. Searches for nine known
class-16 signatures across all 79 CATPart files find 17 raw hits in four
files and none in CTC05. Those raw counts bound known signatures; the closed
standard row and native frames establish the absence of an inline cache.

The CTC05 cylinder at physical offset 606,682 closes in 103 bytes, including
its five-byte frame header, with no persistent identity lane. Independent
parsing verifies the complete pcurve jets at offsets 608,198 and 609,351.
Their support tags 126 and 113 differ, but their five paired sites agree
within `5.707901599824335e-11` mm through opposite cylinder charts. The
tag-113 endpoints select exactly one of 403 native edges at a `2e-3` mm gate:
edge 174, with residual `6.105067919430197e-6` mm. The tag-125 pcurve at offset
645,000 and unique circle ID 125 at offset 515,727 share one descriptor
extent; their U ranges differ and their endpoint V difference is
`9.769569662968705`. Numeric uniqueness fails the circle-chart invariants.

An independent sweep reconstructs logical descriptor sources in all 79
CATPart files, suppresses nested markers, and deduplicates physical starts.
Its width-1/2/3 class-20 frame candidates contain 5,906 valid compact operand
fronts; five additional fronts fail compact decoding. These counts classify
framing and operand fronts, rather than semantically admitted pcurves.
None of the 5,906 operands equals a candidate identity-free carrier offset.
Header-token comparison also fails as a general selector:

| Candidate header representation | Absent | Unique numeric match | Multiple numeric matches |
| --- | ---: | ---: | ---: |
| Packed native token bytes | 5,743 | 62 | 101 |
| Decoded compact token | 5,440 | 221 | 245 |

The sweep retains 623 numeric pcurve/circle collisions. It finds 379 operand
values in multiple documents, with one value used in nine files. No matching
scalar assigns a persistent tag to an identity-free carrier.

The CAA geometry-object documentation defines persistent tags within explicit
or implicit geometry factories, and the creation example reads a pcurve's
underlying surface tag with `GetSurface()->GetPersistentTag()`.
[Geometry objects](https://www.maruf.ca/files/caadoc/CAAGobTechArticles/GeoObjects.htm)
and [geometry creation](https://www.maruf.ca/files/caadoc/CAAGobUseCases/CAAGobCreation.htm)
establish the API namespace. The serialized factory relation remains in SN-12.

Independent logical-stream reconstruction covers all 79 CATPart files and
validates the complete standard population relation:

| Raw population result | Count |
| --- | ---: |
| Primary nested directories | 76 |
| Maximal FBB runs | 83 |
| Complete trim/edge/vertex FBB layouts | 77 |
| Surface-record candidates | 29,651 |
| Source-closed surface/support populations | 75 |
| Admitted positional pairs | 75 in 71 files |
| Paired bounds-containment failures | 0 |
| Unmatched complete FBB layouts | 2 |
| Unmatched surface/support populations | 0 |

The unmatched layouts belong to `steering-wheel-connector.CATPart` and
`steering-wheel.CATPart`. Each has 50 faces, 128 edges, and 80 vertices.
Their complete logical streams contain two and eleven isolated surface-record
candidates, respectively, with no source-closed surface/support population.
The connector and wheel share identical 400-byte FBB face rows and a
1,203-byte vertex table. Their trim chains differ in 22 bytes and edge tables
in nine bytes. The connector's maximal E5 walk has 1,137 frames; the wheel
has two maximal walks of 1,217 frames. The E5 route supplies their geometry.

Fresh native and sibling STEP decodes of the connector, middle horn, outer
ring, complete wheel, and lower Carter produce ten checked documents. Unique
body matches have bijective vertex matches, identical physical-edge endpoint
multisets, and identical per-face loop endpoint multisets at a `0.002` mm
coordinate gate. The four steering specimens contain eight bodies, 226 faces,
232 loops, 1,128 coedges, 564 edges, and 352 vertices, with maximum vertex
residual `7.75264730289162e-6` mm. Their curve and surface family counts agree.

The lower Carter's three standard populations have face/edge/vertex counts
`245/601/350`, `9/18/12`, and `9/18/12`. The latter two share identical FBB
face-row bytes and lie on opposite sides of Y=0. Source-order pairing has no
bounds-containment failures; swapping those two populations causes two.
Each population selects one sibling STEP body with matching topology. This
comparison permits exporter refitting and surface deduplication; it checks
coordinates and incidence independently of carrier-family counts.

An independent 79-file source sweep verifies 27,231 analytic-record
parameter-length words and 40,085 rows in complete surface-owned support
tables. The analytic word is big-endian at record offset `+3`; the support
word is big-endian at row offset `+4`. The analytic length includes the
two-byte parameter prefix and excludes the kind byte. The first three
trimmed-face bounds values are the AABB center, including the point used by
the admitted cylinder and torus branch rules.

A broader center-azimuth rule conflicts with 75 cone and 38 sphere face
witnesses on already checked, branch-bearing standard circles. Those counts
compare against emitted intervals. Independent raw STEP verification proves
four cone conflicts in `collet.CATPart`: the selected arc midpoint agrees
within `4.506e-7` mm, and the center-selected complement misses by 11.5 mm.
The broader rule remains outside decoder admission; additional source-bound
branch relations remain in SN-15.

Independent framing and allocation walks cover all 79 CATPart files and
verify the fixed-nine owner census:

| Reference encoding | Fixed-nine packets |
| --- | ---: |
| All-compact | 749 |
| Tagged alternating | 112 |
| Width-coded alternating | 107 |
| Total | 968 |

The 78 checked documents retain 951 of these packets, with bit-exact binary64
rectangles and binary32 boxes. `apartman.CATPart` adds 17 all-compact packets
verified directly from source. All 45 owner charts have complete seven-frame
logical adjacency and four side relations equal to the stored rectangle:
13 rolling-ball charts, one cylinder chart, and 31 torus charts. Among the
906 decoded packets without charts, 577 resolve backward references; their
1,521 targets comprise 327 vertex and 1,194 edge allocations, with no surface
target. The inline rectangle remains retained without a carrier binding.

An independent A5 knot, pole, weight, and complete-tail parser evaluates
uncharted tagged and width-coded rectangles. Domain comparisons allow
`0.002` parameter units and Cartesian comparisons allow `0.002` mm;
evaluation uses the rectangle's intersection with the candidate domain.

| Candidate result | Tagged owners | Width-coded owners |
| --- | ---: | ---: |
| Owners tested | 112 | 77 |
| No domain candidate | 0 | 20 |
| No four-corner candidate | 0 | 62 |
| No 25-by-25 grid candidate | 0 | 63 |
| One grid candidate | 72 | 12 |
| Multiple grid candidates | 40 | 2 |

The spring owner at offset 40,482 stores its complete 62-byte tail inline.
Carriers at offsets 50,739 and 52,715 pass the corner-box check. A 101-by-101
interior grid has zero coordinate violations on the first and 13,284 on the
second. The second carrier's U domain falls short of the rectangle by
`0.0010119222600337707` parameter units; this comparison clips that narrow
excess and still rejects its interior. Corner containment supplies a candidate
filter, rather than a carrier identity or a full-image proof.

All 31 explicitly bound torus rectangles pass independent 513-by-513 grids,
covering 8,158,239 sites. Minimum lower-side and upper-side Cartesian slack
are both `0.0999755859375` mm. The CTC05 chart at owner offset 603,047 has
two matching torus loci in the checked sibling STEP document, with opposite
reference directions. The stored envelope is conservative and does not select
one surface identity; its observed padding is not a required format constant.

The [CGM surface documentation](https://www.maruf.ca/files/caadoc/CAAGobTechArticles/Surfaces.htm)
separates surface-relative limits from Cartesian bounding boxes, and the
[foreign-surface contract](https://www.maruf.ca/files/caadoc/CAAGobTechArticles/Attribute.htm)
defines boxes for limited parts of surfaces. That API distinction supports
the Cartesian envelope interpretation; it does not identify a serialized
carrier or prove which API method produced the cache.

Independent source parsing verifies 765 ordinary fixed-owner/node joins in
the 78 checked documents. Every node is immediately followed by its owner,
every target's checked successor equals the ninth owner identity, and every
terminal satisfies the admitted grammar. None of those targets or successors
equals a standard carrier tag. Only 93 targets fall within the file's
standard face-ordinal range; 97 target instances repeat. The 18 files with
both standard faces and fixed owners have unequal population counts.
The checked documents contain 12,897 faces in the standard namespaces across
68 files; this count includes every emitted standard population.

The spring and eyebolt four-edge preludes reproduce their local simple
cycles and ninth-identity-plus-ten relations. Their eight curve references
have no numeric match among standard edge-support tags. Two other owner/tag
equalities occur in eyebolt owner 67,398: slots zero and four equal carrier
tags 1,389 and 1,393, whose cylinder geometries are identical. Those equalities
do not select an owner face ordinal.

A separate 79-file byte sweep finds 14 complete eight-reference class-37
constructions: ten in spray bottle and four in NIST CTC02. Nine have controls
`11 09 03 03`, and five have `11 09 05 05`. Each has eight references, eight
zero bytes, terminal `01 05`, and a backward-distance-one first reference.
Logical-source reconstruction verifies a preceding rolling-ball carrier for
all 14, including a carrier split across physical extents. Their 99 stored
sites satisfy the two-radius and rolling-angle relations independently.
The ten all-width tails have the observed pcurve-pair difference two and
three-reference pattern `[base+3,base,base+2]`; the four backward-addressed
tails remain in their own addressing form. One construction closes the
current fixed-owner chart relation. The other complete payloads do not imply
fixed-nine recognition or a new neutral construction.

Fresh sibling STEP documents for spring and eyebolt pass model checks.
Independent vertex, edge-endpoint, and face-loop multiset comparisons match
their native topology at `0.002` mm. Spring has 84 faces, 166 edges,
332 coedges, and 84 vertices; eyebolt has 134 faces, 268 edges, 536 coedges,
and 136 vertices. Joint weighted face-adjacency refinement gives 134 unique
eyebolt face matches and 42 two-face spring classes. Exact adjacency
isomorphisms verify both graphs; the spring mapping remains non-unique.

The [CGM generic-fillet description](https://www.maruf.ca/files/caadoc/CAAGobTechArticles/Surfaces.htm)
defines center and two limiting loci. It does not define the bridge's wire
order. The hand bridge's last three references lack retained alias targets,
so their proposed locus labels remain unassigned under SN-18.

Independent class-`18` verification uses logical descriptor framing,
compact operands, finite lanes, and complete control closure:

| Population | Count | Scope |
| --- | ---: | --- |
| CATPart inputs | 79 | Both corpus roots |
| Files with candidates | 34 | Header token 5, finite compatible lane |
| Admitted records | 26,398 | Complete control closure |
| Inline operands | 15,097 | One-byte `4n+1` |
| u8 operands | 3,408 | `04` and one integer byte |
| u16 operands | 6,237 | `08` and two integer bytes |
| u24 operands | 1,656 | `0c` and three integer bytes |
| u32 operands | 0 | Established grammar; synthetic regression |
| Other-control abstentions | 112 | Finite length-compatible candidates |
| Distinct operand values | 4,855 | Range 1 through 1,727,824 |
| Previous native records | 4,507 | The 78-model baseline, operands 1 through 4 |

No admitted centered start is nonnegative. The maximum explicit direction
norm error is `4.440892098500626e-16`, dimensionless. The raw sweep contains
21,891 records beyond the decoded baseline, including `apartman.CATPart`.

| Four-operand run relation | Runs | Scope |
| --- | ---: | --- |
| Complete contiguous runs | 477 | The 78-model baseline |
| Admitted owner charts | 45 | Carrier/bridge/rectangle context |
| Opposing-side order | 429 | Operand pairs `(1,2)` and `(3,4)` |
| Consecutive-edge order | 48 | The other runs |
| Closed consecutive runs | 47 | Endpoint closure |
| Open consecutive chains | 1 | HTC run at physical offset 806574 |

Endpoint closure alone does not establish owner-chart side order. The FTC
map at 1559012, HTC map at 811022, and basket map at 191651 independently
consume source/target operands `(3,2)`, `(5,4)`, and `(30,29)` from adjacent
class-`18` records.

The complete constructed container passes native validation for all 26,398
admitted records, with exact values, scalar bits, encoding forms, and offsets;
all 112 other-control rows remain excluded. Fresh original decodes and checks
verify 20,259 records in the 33 decodable positive files. Their complete
neutral models remain exact. Existing construction references preserve their
source offsets when the enlarged point arena renumbers IDs. The other 45
inputs have no candidates. `apartman.CATPart` reaches its existing 512-unit
ordered-face constraint refusal; its 6,139 source records are verified in the
constructed container. The local verification harness uses a 128-million
collection-item ceiling for the FTC10 native serialization.

The public CGM articles [How to Associate Topology with Geometry](https://library.plmcoach.com/caadoc/online/CAACgmModel/CAACgmTaTobTopoCreate.htm)
and [Using the CGM Data Checker](https://www.maruf.ca/files/caadoc/CAACgmTechArticles/CAACgmDataChecker.htm)
describe multiple curve representations per edge and parameter mappings
between their domains. These object roles support the representation context;
they do not assign serialized class numbers or join bare operand values.
