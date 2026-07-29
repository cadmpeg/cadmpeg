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

- Structurally complete object graphs retain design objects, ordered fields,
  exact field classes, definition-bound values, and inter-object reference
  occurrences.
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
  tapes form radial physical-edge candidates. Their endpoint coordinates form
  geometric vertex candidates only when each coincidence component is a
  complete pairwise clique within tolerance; ambiguous tolerance chains remain
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
- Every consolidated line profile transfers its placed analytic line carrier.
  A non-unit profile retains its metric scalar and interval while its neutral
  parameter mapping remains unresolved.
- A standard binary32 cylinder, cone, sphere, or torus face carrier is refined
  to its complete consolidated binary64 frame when quantization selects exactly
  one same-family record. One exact carrier may refine repeated face carriers;
  missing and multiply matching records leave the standard carrier unchanged.
- The freeform fallback transfers complete consolidated cylinder, cone, sphere,
  and torus records as placed analytic surface carriers independently of
  unresolved topology ownership.
- Zero-entity surface carriers retain their complete face-local `21xx` support
  tapes. Every occurrence keeps its framed local slot, record family, and
  inline UV endpoint pair when that family stores one. The independent `5e1a`
  edge-stride, `2569`/`0638` positional-use, and counted `05xx`
  vertex-incidence registries retain one-based global record ordinals and exact
  references against a complete framed-record identity inventory. Each
  edge-stride atomically binds its two typed adjacent support records. Each
  counted vertex-incidence record binds its immediately following `5d06`
  vertex-owner record. Supports with inline UV endpoints lift those endpoints
  through their exact plane, cylinder, circular-cone, torus, or NURBS owner
  carrier into model space.
  A complete `5fxx` face roster binds positionally to those support tapes and
  retains each face's counted allocations and ordered loop terminals.
  Complete `62xx` loop rosters bind to those terminals and retain alternating
  logical-member and typed-reference lanes, loop class, and absolute member
  senses. Each loop member binds to the unique face-local support occurrence
  whose slot equals the loop terminal minus that member; the complete
  face-local binding is atomic and retained as support-record ordinals. A loop
  retains its complete sense-oriented model endpoint tape when every support
  lifts directly or exactly one missing pair is bounded by lifted neighbors
  and every cyclic join closes within the format tolerance. Coincident
  occurrence pairs establish face-incidence components only when each
  occurrence has one geometric match. Coincident groups partition by those
  components, and every two-occurrence partition retains one physical-edge
  candidate. Every
  in-range odd-lane typed reference retains its selected global record identity
  atomically for the loop.
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
