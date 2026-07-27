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

| Level | Required evidence | Current result | Remaining gate |
| --- | --- | --- | --- |
| L0 | Signature and part-kind detection; bounded container metadata; preview or tessellation when present | Pass in implementation | Representative release-band fixtures |
| L1 | Section/stream navigation; ND and DEPDB dispatch; bounded Unix-compress expansion; version/layout reporting; named opaque sections | Claimed | Close the release and layout envelope and verify every admitted section combination |
| L2 | Placed points; analytic and NURBS curves and surfaces; correct units and parameterization across the envelope | Incomplete | Remaining positional curve and surface bodies, prototype-instance joins, spline joins, type-26 placements, and lane-specific scalar forms |
| L3 | Connected bodies through vertices with ownership, orientation, trimming, placements, and transforms; unknown carriers permitted | Incomplete | Complete face-instance partitioning, rowless face-use binding, loop classification, vertex coordinates, and shell-to-body ownership |
| L4 | Typed feature operations, sketches, ordering, dependencies, profiles, directions, and extents | Incomplete | Resolve the remaining operation families and incomplete operands, including chamfer, draft, mirror, boundary, merge, fill, thicken, and non-default sweep termination |
| L5 | Every admitted carrier and topology case; typed mainstream bodies throughout; body and face colors | Incomplete | Close all L2/L3 families, transfer appearance bindings and precedence, then demonstrate zero shape-domain loss across the envelope |
| L6 | Complete constraints, dimensions, parameters, expressions, feature semantics, configurations, and coherent re-derivation history | Incomplete | Complete solver relation/incidence families, dimension-variable joins, expressions, every admitted feature family, configuration driver tables, and history replay coherence |

## Implemented design slices

- Saved planar sections transfer placed sketch points, lines, arcs, splines,
  dimensions, and typed or identity-preserving native constraints.
- Active solver incidences drive coordinate, orientation, equality, radius,
  and supported dimensional equations; disabled incidences remain retained but
  do not affect solved geometry.
- Linear extrusions and rotations transfer when profile, placement, direction,
  and termination have independent byte-backed proofs.
- Holes and rounds transfer typed operation definitions where their affected
  geometry, edge identities, radii, and extents resolve uniquely.
- Curve-equation assignments retain source order and dependency identity;
  closed numeric and string operator and deterministic function values transfer,
  including exact and regular-expression whole-string matching.
  Local bindings are case-insensitive, scoped external symbols remain whole,
  and the reserved `PI` and dimensioned gravitational `G` constants evaluate.
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
  validity and typed angular results.
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
  distinct producing feature, independent of feature-row order.
- Bare `Body`, `Körper`, and `Surface` operation states without a recipe,
  schema row, or feature reference name transfer as solid-body and surface-body
  model-tree nodes. Recipe-, schema-, and reference-backed records retain their
  modeling-operation precedence.
- Every decoded section-dimension row transfers as a definition-scoped design
  parameter; table completeness gates ordinal relation joins, not row
  preservation.
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
  that native line and point locus are both emitted.
- A type-four incidence with one sense-zero line or arc and one
  endpoint-selected operand transfers as an explicit tangent-loci constraint
  when the selected section-point identifier matches exactly one endpoint of
  the sense-zero entity. This structural join is independent of solver
  activity and does not require evaluated tangent vectors.
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
  Solver activity controls constraint activity, not the stored arc role or
  endpoint selection.
- Two-locus type-fifteen incidences transfer the same flag-selected
  same-coordinate constraint as type seventeen. Unsupported flags remain
  native.
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
   Duplicate native identifiers are counted separately as ambiguous rows.
   Nonzero untransferred and ambiguous row counts each raise a decode loss note.
   The coverage map separately counts decoded, transferred, typed, and native
   `relat_ptr` and `skamp_ptr` constraints, with active typed and native
   partitions. Every active native constraint raises a decode loss note.
   It also counts all transferred history features, partitions their
   definitions into typed and native forms, and separately counts typed
   definitions whose model-space construction is explicitly unresolved. Every
   explicit unresolved definition raises a decode loss note.
4. Validate semantic fingerprints for units, placements, carrier parameters,
   connected topology, feature order, dependencies, sketches, constraints,
   dimensions, expressions, and configuration state. The coverage map counts
   decoded and transferred section dimensions separately and counts dimensions
   whose scalar values resolve. It counts decoded section solver variables,
   dimension-driven sentinel variables, and dimension-driven variables whose
   exact ordinate resolves through the complete equation system separately. It
   likewise counts decoded, transferred, and
   evaluated active curve-equation assignments separately and partitions them
   by active, inactive, and unresolved-conditional state. Prohibited active
   records and their distinct prohibited construct kinds are counted separately,
   and each nonzero prohibited count raises a decode loss note. Container and
   census facts about the file — version line, layout, section table, namespace
   array sizes, principal unit, family-table pointer, and configuration state —
   remain in the source metadata attribute map.
5. Run malformed-input and fuzz gates for every admitted parser family.

The current public score remains L1 claimed. Capabilities above L1 are extras
until every cumulative gate through their level passes for a closed envelope.
