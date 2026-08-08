# Creo Parametric `.prt` coverage

This document applies the cumulative support ladder to the Creo PSB reader.
It records implementation coverage and verification gates. Byte semantics
belong in [creo_prt.md](creo_prt.md); unresolved byte meanings belong in
[creo_prt-open-items.md](creo_prt-open-items.md).

## Envelope

The implemented format band is `#UGC:2` PSB part documents using the ND or
DEPDB section layouts recognized by the container scanner. This is not yet a
closed support envelope: supported Creo release bounds, required and optional
section combinations, and the admitted geometry and feature-family matrix have
not been fixed. Until that matrix is closed and exercised by representative
fixtures, claims above L1 remain unproven.

## Cumulative gates

| Level | Required evidence                                                                                                                  | Current result         | Remaining gate                                                                                                                                                               |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| L0    | Signature and part-kind detection; bounded container metadata; preview or tessellation when present                                | Pass in implementation | Representative release-band fixtures                                                                                                                                         |
| L1    | Section/stream navigation; ND and DEPDB dispatch; bounded Unix-compress expansion; version/layout reporting; named opaque sections | Claimed                | Close the release and layout envelope and verify every admitted section combination                                                                                          |
| L2    | Placed points; analytic and NURBS curves and surfaces; correct units and parameterization across the envelope                      | Incomplete             | Remaining positional curve and surface bodies, prototype-instance joins, spline joins, type-26 placements, and lane-specific scalar forms                                    |
| L3    | Connected bodies through vertices with ownership, orientation, trimming, placements, and transforms; unknown carriers permitted    | Incomplete             | Complete face-instance partitioning, rowless face-use binding, loop classification, vertex coordinates, and shell-to-body ownership                                          |
| L4    | Typed feature operations, sketches, ordering, dependencies, profiles, directions, and extents                                      | Incomplete             | Resolve the remaining operation families and incomplete operands, including chamfer, draft, mirror, boundary, ambiguous surface-merge quilt-to-surface joins, and non-default sweep termination |
| L5    | Every admitted carrier and topology case; typed mainstream bodies throughout; body and face colors                                 | Incomplete             | Close all L2/L3 families, transfer appearance bindings and precedence, then demonstrate zero shape-domain loss across the envelope                                           |
| L6    | Complete constraints, dimensions, parameters, expressions, feature semantics, configurations, and coherent re-derivation history   | Incomplete             | Complete solver relation/incidence families, dimension-variable joins, expressions, every admitted feature family, configuration driver tables, and history replay coherence |

## Implemented design slices

- Saved planar sections transfer placed sketch points, lines, arcs, splines,
  dimensions, and typed or identity-preserving native constraints.
- Section-reference lines transfer as construction-line geometry when both
  stored endpoint references resolve to distinct section coordinates.
- Section lines with a uniquely proven fixed coordinate and unresolved
  along-line coordinates remain unbounded through solver intersections and
  construction-line transfer; no finite endpoint extent is inferred.
- Section-reference lines use their stored selector or a consistent
  perpendicular/parallel incidence component to establish the same unbounded
  fixed-coordinate construction when both referenced endpoint ordinates agree.
- Active solver incidences drive coordinate, orientation, equality, radius,
  and supported dimensional equations; disabled incidences remain retained but
  do not affect solved geometry.
- Linear extrusions and rotations transfer when profile, placement, direction,
  and termination have independent byte-backed proofs. Additive linear
  extrusions also accept a closed one-entity circle section, a closed single
  full-turn arc section, and closed profiles containing interpolation-spline
  entities. Circular profiles preserve their complete geometry through area,
  cap p-curves, side p-curves, and edge-parameter derivation. Spline profiles
  preserve their intrinsic NURBS degree, knot vector, control points, weights,
  cap p-curves, edge parameter domains, and ruled side surfaces.
- Full-turn revolutions accept interpolation-spline profile entities and
  preserve their oriented directrix degree, intrinsic knot domain, poles,
  weights, periodic flag, exact angular NURBS surface, constant-parameter
  endpoint p-curves, and face sense.
- Full-turn revolutions reconstruct a missing section axis from their complete
  transferred carrier surfaces, requiring coaxial analytic axes, parallel
  carrier-plane normals, and carrier-sphere centers on the common axis.
- Class-942 `Surface` operations with a unique numbered `Extrude` reference
  transfer as independent sheet linear sweeps; the same class without that
  reference remains distinct from the sheet-sweep family.
- Generated cap-plane tables and complete positional-cylinder carriers provide
  blind extrusion spans to generated feature surfaces and first additive linear
  or one-circle B-reps when the section transform agrees with the carrier
  direction. Complete NURBS-translation carriers provide the same span to
  generated NURBS feature surfaces when their profile is supported.
- Axis-aligned coaxial cone-cylinder intersections transfer the unique circle
  whose axial center coordinate agrees with the repeated `fc 14` held
  world-coordinate token.
- Holes and rounds transfer typed operation definitions where their affected
  geometry, edge identities, radii, and extents resolve uniquely.
- A round whose generated type-26 rows all replay the same uniquely associated
  prototype minor radius transfers that exact value as its constant radius;
  patch placement is independent of the radius proof.
- Curve-equation assignments retain source order and dependency identity;
  closed numeric and string operator and deterministic function values transfer,
  including exact and regular-expression whole-string matching.
  Local bindings are case-insensitive, scoped external symbols remain whole,
  and the reserved `PI` and dimensioned gravitational `G` constants evaluate.
  Simultaneous `SOLVE`/`FOR` blocks retain ordered equation sides,
  dependencies, block-local auxiliary assignments, and unknowns. Auxiliary
  assignments retain ordinary target, namespace, dependency, and evaluation
  semantics; simultaneous equations do not become sequential assignments.
  Complete dimensionally valid affine systems over previously valued numeric
  unknowns evaluate in canonical relation units when they have one finite
  consistent solution. Fixed Boolean annihilators, equal conditional branches,
  signed zero, and identity or zero powers reduce within affine equations even
  when the controlling operand remains unknown. Unknowns may have different
  physical dimensions, and
  known dimensioned coefficients participate in the affine system. Nonlinear,
  dimensionally inconsistent, dependency-unresolved, underdetermined, and
  inconsistent systems retain absent aligned solution values.
  Constructs prohibited in datum-curve equations are retained but do not
  evaluate or generate a derived curve. Positive
  `exists()` queries resolve against the complete local assignment namespace
  and decoded `d<external_id>` section-dimension identities. Unambiguous
  decoded dimension values initialize those relation symbols in millimeters or
  degrees; conflicting and unresolved occurrences remain symbolic. Explicit
  length, area, volume, mass, time, force, energy, power, pressure, angle, and
  temperature units convert to canonical relation units and compound exponent
  vectors propagate through dimensionally valid arithmetic. Celsius and
  Fahrenheit apply affine conversion before evaluation. Length and angle
  results transfer as typed neutral values; other dimensions remain evaluated
  native values because the neutral parameter model has no corresponding scalar
  types.
  Conditional selection, range and deadband functions, sign and remainder,
  rounding, tolerance tests, and trigonometric results preserve dimensional
  validity and typed angular results. Integer and real string conversion accepts
  every numeric scalar in canonical relation units while keeping formatting
  controls dimensionless.
  Context-dependent cabling, case-study, graph, trajectory, mass-property, and
  material functions and series/list parameter queries retain their argument
  dependencies without treating the function name as a parameter; their values
  remain symbolic until the referenced namespace is decoded.
  Session-scoped `rel_model_name:<session_id>()` calls likewise remain symbolic
  model-context queries without becoming parameter dependencies.
  Colon-scoped assignment targets retain their complete typed identity and
  source-order value semantics without emitting false local parameters.
  Unscoped dimension, tolerance, and pattern targets retain their typed system
  namespace and source-order value semantics without emitting user parameters.
  Registered function-write targets retain their name, ordered argument
  expressions, and dependencies without emitting parameters.
  Left-hand `value(parameter,row[,column])` statements retain a typed table-cell
  target and its complete dependency order without emitting a false scalar
  parameter.
  Unit declarations on newly created assignment targets define typed parameter
  values and remain separate from parameter identity.
  A unique transferred dimension identity becomes the neutral parameter
  dependency; duplicate identities remain source metadata. Other namespaces
  remain unresolved. Affine
  cylindrical-coordinate programs transfer as helices.
- Feature rows, parent/input tables, affected geometry and edge identifiers,
  recipe effects, saved sections, and operation states retain stable native
  identities when neutral semantics remain incomplete. A class-100 generated
  entity reference adds a history dependency when that entity has exactly one
  preceding feature-generated class-200 producer.
- Unique feature-owned class-200 materialized surfaces now emit feature-result
  topology face identities. Hole placement, thicken inputs, and knit inputs
  use generated face references only when those identities and their producer
  dependencies are declared; ambiguous, rowless, or foreign-owned surfaces
  remain native selections.
- Unique feature-owned `crv_array` topology rows now emit feature-result edge
  identities. Fillet and chamfer affected-edge selections use generated edge
  references only when the topology row is unique and its producer result
  declares the matching local edge identity. The coverage map counts both
  result-topology states and their declared result edges.
- Feature-local pre-rollback, post-rollback, and post-regeneration outlines
  retain each of their six exact scalar bodies independently of numeric decode.
- Bare `Body`, `Körper`, and `Surface` operation states without a recipe,
  schema row, or feature reference name transfer as solid-body and surface-body
  model-tree nodes. Recipe-, schema-, and reference-backed records retain their
  modeling-operation precedence.
- Every decoded section-dimension row transfers as a definition-scoped design
  parameter; table completeness gates ordinal relation joins, not row
  preservation. Each row retains the exact bounded scalar bodies for its primary
  and auxiliary values. Decoded dimensions whose primary scalar semantics remain
  unresolved retain the source-native value token and raise a decode loss note.
- Section-segment coverage counts decoded rows, rows with resolved neutral
  geometry, decoded rows retaining source-native geometry, and declared rows
  that did not decode. Each nonzero unresolved or missing count raises a decode
  loss note. A uniquely joined evaluated saved entity resolves its corresponding
  generic bounded-curve or saved-conic segment row as well as the emitted sketch
  entity.
- Section-coordinate solving accepts a complete variable table or no variable
  table. An incomplete variable table contributes no coordinate equations;
  missing declared rows are counted and raise a decode loss note. Complete
  variable rows retain the exact encoded value and pre-solve guess bodies
  independently of scalar interpretation, including independent
  dimension-driven sentinel state for each lane. Complete endpoints from
  uniquely joined saved lines and arcs, and the center of a joined saved arc or
  circle, seed the corresponding segment-point equations even when no variable
  table is present. A joined saved circle also seeds its
  radius-reference component. Disagreement with stored or constraint-derived
  coordinates or radii withholds the inconsistent derivation.
- Constraint coverage separates typed and native `skamp_ptr` incidences and
  `relat_ptr` relations by discriminator, including the active native subset.
  It also counts decoded and missing declared relation, incidence, and
  relation-incidence join rows. A zero relation allocation count is counted
  separately. Diagnostics report every nonzero row shortfall, malformed
  relation allocation, and active native discriminator.
- Every decoded non-null `segtab.verhor` field transfers as a distinct source
  constraint. Values zero and one on a line use the defined neutral vertical
  and horizontal forms; other segment families and selector values retain the
  exact scalar and segment identity in a native constraint.
- Every decoded non-null primary and secondary `segtab` radius field transfers
  as a distinct source constraint with its segment identity, field role, and
  dimension ordinal. A uniquely identified type-10 circle whose primary field
  resolves to a type-three or type-four dimension uses the neutral radius or
  diameter form; all other bindings remain native.
- A complete endpoint-selection or type-35 incidence whose non-target operand
  resolves to a point locus establishes its sense-zero operand's line role
  independently of solver activity. A unique unary type-one or type-two
  incidence can therefore transfer as a neutral horizontal or vertical
  constraint on that native line without activating the corroborating equation.
  The type-35 incidence itself transfers as a neutral midpoint constraint when
  that native line or arc and point locus are both emitted. Resolved line and
  centered-line targets add affine midpoint equations; resolved arc targets add
  their oriented analytic midpoint after the center and endpoints are known.
  A sense-zero circular operand supplies its center as the midpoint locus; an
  unresolved centered type-47 line supplies its stored sense-four center
  without becoming a bounded midpoint target or acquiring line coordinates.
- A type-four incidence with one sense-zero line or arc and one
  endpoint-selected operand transfers as an explicit tangent-loci constraint
  when the selected section-point identifier matches exactly one endpoint of
  the sense-zero entity. This structural join is independent of solver
  activity and does not require evaluated tangent vectors.
- A two-item type-five incidence transfers as entity-level perpendicularity
  when both sense-zero operands have uniquely established curve families.
  Line-only coordinate-orientation propagation remains restricted to two
  uniquely established lines.
- A type-37 incidence transfers as a projected-copy relation when its two
  sense-zero operands are consecutive reference/result identities and the
  result has a unique row in the trimmed profile.
- A sense-four incidence item establishes a solver-only entity's circular
  family independently of solver activity. A disabled type-three incidence can
  therefore transfer its selected center onto a sense-zero curve. A disabled
  type-three incidence between two emitted sense-zero point entities transfers
  as coincident loci.
- Unary type-ten and type-eleven incidences on a uniquely established arc
  transfer as neutral 90-degree and 180-degree fixed arc-angle constraints.
  Solver activity controls constraint activity, not the stored arc role or
  fixed angle.
- Unary type-twelve and type-thirteen incidences on a uniquely established arc
  transfer as horizontal and vertical alignment of the arc endpoint loci.
  Active forms also add the corresponding endpoint-coordinate equality to the
  affine solver; inactive forms retain the neutral constraint without adding
  an equation. Solver activity controls constraint activity, not the stored
  arc role or endpoint selection.
- Two-locus type-fifteen incidences transfer the same flag-selected
  same-coordinate constraint as type seventeen. Disabled forms retain
  endpoint-selected loci on emitted solver-only carriers without requiring a
  solved section-point identity. Unsupported flags remain native.
- Neutral incidence definitions are emitted only when every selected locus is
  compatible with its emitted entity family, regardless of solver activity;
  incompatible active and inactive candidates remain native constraints.
- A native `relat_ptr` constraint retains each decoded non-null `a`, `b`, and
  `c` operand at its fixed vector slot. Null slots remain absent rather than
  becoming zero-valued object references. Native `relat_ptr` and `skamp_ptr`
  constraints retain the complete stored `used` and solver-status values,
  respectively. Native `relat_ptr` constraints retain the stored sign and
  dimension selector as scalar properties distinct from object-reference
  operands. Native `skamp_ptr` constraints also retain the complete stored flags
  separately from solver status. Each ordered incidence item retains its owning
  sketch namespace and `items.entity_id` field with its entity identifier; the
  item sense is its numeric native role. An incomplete or duplicate incidence
  identity remains an exact scalar property. The known status low bit is
  projected as constraint activity. A native `relat_ptr` constraint retains its
  `triples_ptr.skamp_id` incidence link when the relation, join, and incidence
  identities are complete and unique. The same unique join retains its
  non-null equation identity without assigning semantics to that namespace. An
  equation-bearing join whose incidence identity and equation-bearing rows are
  complete and unique also retains the equation identity on a native
  `skamp_ptr` constraint, including when the join has no relation identifier. An
  incomplete or duplicate relation identity remains a scalar property and does
  not become an ambiguous relation-record operand.

## Evidence required to raise the score

1. Declare a finite release/layout/feature matrix for the primary envelope.
2. Manifest representative fixtures for every admitted matrix cell, including
   negative and ambiguity cases.
3. Record per-fixture geometry, topology, design, and configuration loss
   expectations and require no blocking loss through the claimed level.
   The decode report's coverage map records unique, transferred, and
   untransferred visible surface- and curve-row counts. Surface counts are
   partitioned by family; curve counts are partitioned by raw type byte because
   the curve namespace does not independently define geometric families.
   Every unique row without a transferred carrier retains an explicit unknown
   carrier linked to its containing native geometry record. The map counts
   these retained unknown carriers separately, including the same surface-family
   and curve-type partitions; they remain untransferred for shape-domain
   coverage.
   Duplicate native identifiers are counted separately as ambiguous rows.
   Nonzero untransferred and ambiguous row counts each raise a decode loss note.
   The coverage map separately counts decoded, transferred, typed, and native
   `relat_ptr` and `skamp_ptr` constraints, with active typed and native
   partitions. It counts decoded `triples_ptr` joins and missing declared rows
   in all three constraint tables. Relation tables with the invalid zero
   allocation count are counted separately. Every missing row, malformed
   relation allocation, and active native constraint raises a decode loss
   note.
   It also counts all transferred history features, partitions their
   definitions into typed and native forms, and separately counts typed
   definitions whose model-space construction is explicitly unresolved. Every
   native definition raises a decode loss note. Geometry rows whose nonzero
   generator identity has no operation, feature row, or datum definition
   transfer as stored-geometry features and are counted separately. The
   unresolved typed count is
   partitioned into datum-plane, datum-coordinate-system, boundary-surface, and
   draft families. Every explicit unresolved definition raises a decode loss
   note. Extrusions are counted separately, with
   unresolved and native profiles, incomplete start-face operands, incomplete
   termination operands, and unresolved Boolean operation partitions.
   Revolutions are counted separately, with missing or unresolved profiles,
   native profiles, missing axes, missing or incomplete angular extents, and
   unresolved Boolean operation partitions. A sweep with any required operand
   in one of these partitions raises a decode loss note. Recognized holes,
   fillets, chamfers, and drafts are counted separately. Hole coverage
   partitions missing location, unresolved and native profiles, unresolved and
   native placement faces, direction, kind, diameter, and incomplete
   termination operands. Fillet and chamfer coverage partitions unresolved and
   native edge selections from unresolved radius or dimensional specifications.
   Unresolved fillet radii are further partitioned by whether the feature owns
   any generated surface row, separating absent radius carriers from generated
   carriers whose radius proof is incomplete.
   Draft coverage
   partitions unresolved and native face selections and neutral planes, pull
   direction, angle, outward sense, and wholly unresolved definitions. A
   recognized feature with any required operand in one of these partitions
   raises a decode loss note. Filled-surface coverage partitions unresolved
   boundaries, support faces, continuity, and merge controls. Knit-surface
   coverage partitions unresolved faces, entity merging, and solid creation.
   Thicken coverage partitions unresolved faces, thickness, and side. Any
   incomplete surface construction raises a decode loss note. Pattern coverage
   partitions unresolved seed selections and transform operands. Analytic
   helices whose axis remains source-native are counted separately. Either
   condition raises a decode loss note.
4. Validate semantic fingerprints for units, placements, carrier parameters,
   connected topology, feature order, dependencies, sketches, constraints,
   dimensions, expressions, and configuration state. The coverage map counts
   decoded section-segment rows, resolved and unresolved segment geometry, and
   missing declared segment rows separately. The decoded row total includes
   ordinary and every typed special segment family and equals the sum of
   resolved and unresolved geometry. It also counts decoded and missing
   solver-variable rows separately. It counts
   decoded and transferred section dimensions separately and counts dimensions
   whose scalar values resolve or remain unresolved. It counts decoded section
   solver variables and dimension-driven sentinel values and pre-solve guesses.
   Dimension-driven values are partitioned into coordinate types one and two,
   whose exact ordinate may resolve through the complete equation system, and
   other solver-variable types whose dimension semantics remain unresolved.
   Every unresolved dimension-driven value or guess raises a decode loss note.
   The map likewise counts decoded, transferred, and evaluated active
   curve-equation assignments separately, counts typed table-cell targets, and
   partitions assignments by active, inactive, and unresolved-conditional
   state. Complete simultaneous-solve blocks, equations, auxiliary assignments,
   and unknowns are counted separately, as are records with malformed or
   incomplete solve control. Prohibited active records and their distinct
   prohibited construct kinds are counted separately. Every nonzero prohibited
   or solve-control count raises a decode loss note. Container and census
   facts about the file — version line, layout, section table, namespace array
   sizes, principal unit, family-table pointer, and configuration state — remain
   in the source metadata attribute map. Referenced configuration driver tables
   are counted separately from transferred configuration tables, and every
   unresolved reference raises a decode loss note.
5. Run malformed-input and fuzz gates for every admitted parser family.

The current public score remains L1 claimed. Capabilities above L1 are extras
until every cumulative gate through their level passes for a closed envelope.
