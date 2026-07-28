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
- Complete numeric parameters and formula relations transfer when their type,
  owner, value, expression, and dependency identities resolve exactly.
- Empty `PRTSketch` and `Sketch` declarations transfer one neutral planar
  sketch identity and its linked ordered sketch-history feature. Exact
  principal-plane declarations resolve the corresponding origin frame.
- Several non-primary envelopes transfer connected topology or exact analytic,
  NURBS, and procedural carrier subsets. These are extras until every
  cumulative gate in one closed envelope passes.

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
