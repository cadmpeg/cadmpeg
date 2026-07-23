# CATIA V5 `.CATPart` Coverage Contract

This contract defines the read envelopes and cumulative support gates for the
CATIA V5 codec. A gate passes only when every admitted row in its envelope
passes. Retaining a native record does not satisfy a neutral semantic gate.

## Envelopes

| Envelope | Included storage layouts |
|---|---|
| Standard nested | Nested `V5_CFV2` with an FBB face spine, standard edge tables, counted vertex table, and positional `SurfacicReps` roster |
| FBB-only | Nested `V5_CFV2` with complete-run FBB edge tables and no standard delimiter |
| Object stream | Reference-closed A8/B5 object graphs |
| E5 | Reference-closed `e5 0d 03` topology graphs |
| Zero entity | Reference-closed `a9 03` topology graphs |
| Contiguous inner body | Nested `V5_CFV2` without a BREP-body directory |

One file may contain records from several envelopes. The governing topology
layout selects the topology envelope; auxiliary records remain required by the
geometry, design, and application gates.

## Cumulative gates

| Level | Required matrix | Passing condition | Current state |
|---|---|---|---|
| L0 | CATPart signature; outer container; saved-by version; document kind; summary preview present/absent | Detection is bounded; container framing closes; version fields and document kind are typed; a stored preview transfers byte-exactly when present | Implemented |
| L1 | Outer and nested directories; physical and reconstructed logical streams; FINJPL blocks; external document references; every admitted layout discriminator | Every directory and stream extent is bounded; compression and reconstruction are deterministic; embedded and external assets retain identity; undecoded bounded content is named and retained | Implemented for decoded layout bands |
| L2 | Points; lines; circles; conics; analytic surfaces; NURBS curves and surfaces; exact procedural carriers; units, parameterization, placement, and trim ranges | Every required carrier is typed or represented by an exact neutral construction; evaluations agree with stored loci and tolerances; no required carrier remains unknown | Incomplete |
| L3 | Solid, sheet, wire, multiple regions and shells, holes, seams, closed edges, non-manifold radial rings, disconnected components, and every admitted topology layout | Bodies connect through regions, shells, faces, loops, coedges, edges, vertices, and points; ownership, orientation, trimming, sharing, placement, and body kind validate without inferred identity | Incomplete |
| L4 | Ordered bodies and feature roots; sketches and construction geometry; feature operations; operands; directions; limits; saved-result links; suppression and update state | Every projected design record has stable native identity, one structural owner, typed operation semantics, ordered dependencies, and exact links to its saved geometry | Incomplete |
| L5 | Every L2/L3 carrier and topology branch; body, face, edge, curve, point, and construction-geometry appearance | The admitted geometry/topology census has no unknown required carrier or topology case; mainstream parts are typed throughout; appearance ownership and precedence validate | Incomplete |
| L6 | Sketch constraints and dimensions; dimensional and non-dimensional parameters; expressions and units; every feature family and branch; configurations; coherent regeneration history | Constraint, parameter, expression, configuration, and complete feature graphs are typed and valid; history order and dependencies can re-derive every saved result; the design-domain loss report is empty | Incomplete |

L4 and L6 are separate gates. A typed feature name without its operands and
operation controls does not satisfy L4. A complete saved B-rep without
re-derivable design history does not satisfy L6.

## Required proof

Each envelope requires fixtures covering every admitted row in the cumulative
matrix. A proof consists of:

1. bounded decode with a closing physical and logical byte ledger;
2. canonical IR validation;
3. exact carrier evaluation and parameter-domain assertions;
4. topology cardinality, ownership, sharing, orientation, and trim assertions;
5. design-object identity, ownership, ordering, operand, and saved-result assertions;
6. zero blocking losses in every domain required by the claimed level.

Conditional success does not raise an envelope score. A topology algorithm that
declines ambiguous files remains useful recovery behavior but does not satisfy
L3 until the admitted topology matrix is complete.

## Open gates

The authoritative unresolved byte semantics are listed in
[`catia-open-items.md`](catia-open-items.md). The dominant cumulative gates are:

- L2: freeform alias binding, standard spline-cache programs, unsupported conic
  pcurves, and remaining persistent carrier references.
- L3: standard endpoint identity, multi-group topology membership, unresolved
  orientation fields, and the contiguous-inner-body topology model.
- L4: typed feature roots, ordered dependencies, sketches, complete operation
  controls, and saved-result bindings.
- L5: closure of the full L2/L3 matrices plus appearance ownership and
  precedence.
- L6: parameters, expressions, dimensions, constraints, configurations, and
  coherent regeneration history for every admitted feature family.
