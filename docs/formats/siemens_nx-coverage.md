# Siemens NX `.prt` Coverage Contract

This contract defines the evidence required for Siemens NX capability claims. A
gate passes only when every applicable row in that level and every lower level
passes across the declared layout and release envelope.

## Envelope

The read envelope is an SPLMSSTR part container with bounded HEADER and FOOTER
directories, NX object-model sections, Parasolid partition and deltas streams,
and optional DisplayJT, XML, external-reference, and material streams. A release
or layout band enters the envelope when its governing versions and record
variants are identified and its physical and logical byte ledgers close.

Assemblies remain in the read envelope. Their component geometry contributes to
the assembly gates, not the single-part body, feature, or sketch gates.
External component dependencies and unresolved occurrence placements are
product-domain losses assessed at L7. They do not constitute missing inline
geometry in the cumulative L0–L6 gates.

## Capability gates

| Level | Required semantics                                                                                                                                                                        | Passing evidence                                                                                                                                                                                                                                           |
| ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| L0    | Detection, document kind, bounded container framing, metadata, and preview or tessellation when present                                                                                   | Every admitted file has one governing container version, a closed physical ledger, and a typed preview or display mesh when the corresponding source stream exists.                                                                                        |
| L1    | Complete directory and stream navigation, compression, checksums, layout versions, embedded assets, external references, and named unsupported content                                    | Every admitted stream has stable identity, ownership, bounds, and classification. No bytes are reachable only through an unbounded scan. Logical ledgers close for every decoded stream family.                                                            |
| L2    | Placed points, analytic curves and surfaces, NURBS, trimmed curves, offset surfaces, blend surfaces, and surface intersections with correct units, parameterization, and model placement  | Every geometry carrier referenced by active topology is typed. Carrier evaluation and inverse parameterization pass the declared document tolerance over finite domains. Procedural branches are selected by source witnesses rather than proximity alone. |
| L3    | Connected bodies through vertices, including regions, shells, faces, loops, fins, edges, ownership, orientation, trimming, transforms, revision replay, and active-body selection         | Every current body has one connected, validating topology graph. Delta replacement and deletion resolve every active reference. Multi-partition history assigns one terminal lineage state to every emitted body image.                                    |
| L4    | Ordered feature history, typed operations, complete operands and outputs, sketch geometry and profiles, parameters, expressions, dependencies, configurations, and suppression state      | Every active history operation has a typed family, ordered inputs, complete construction fields, valid dependencies, and coherent output lineage. Every sketch used by a feature has a placed neutral graph.                                               |
| L5    | Every carrier and topology branch in the envelope plus body and face appearance                                                                                                           | The geometry and topology censuses contain no required unknown family or case. Exact topology validates without healing. Body and face color/material ownership and precedence are typed. Shape-domain loss is empty.                                      |
| L6    | Complete sketch constraints and dimensions, parameters and expressions, every feature family with full operation semantics, configurations, and history sufficient to re-derive the model | Constraint, expression, configuration, and feature graphs validate. Re-derivation produces the saved current-body census and all projected operation outputs. Design-domain loss is empty.                                                                 |

## Current gate state

| Gate | State                        | Required closure                                                                                                                                        |
| ---- | ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| L0   | Pass in the current envelope | Representative release and layout fixtures with closed physical ledgers.                                                                                |
| L1   | Pass in the current envelope | Representative release and layout fixtures with closed logical ledgers for every classified stream family.                                              |
| L2   | Pass                         | The declared geometry carriers, parameterization rules, and witnessed terminal branches pass the current envelope.                                      |
| L3   | Pass                         | Connected topology, revision replay, active-body selection, and the available terminal lineage pass the current envelope.                               |
| L4   | Incomplete                   | Resolve suppression, sketch placement and entities, complete operands and outputs, configuration body state, and every admitted operation construction. |
| L5   | Incomplete                   | Body/face appearance evidence passes; the cumulative gate remains pending until the lower L4 design-domain gate closes.                                 |
| L6   | Incomplete                   | Close sketch constraints and dimensions, all expression forms, every feature construction, inactive configurations, and re-derivation coherence.        |

The codec public score is L2. The selected-body and terminal-lineage
validation profile passes cumulatively through L3 and shows as extras.
Profile version 10 records saved-body re-derivation as separate L6 evidence,
identifies the first unsupported feature family and history ordinal,
aggregates exact loss messages and re-derivation boundaries, discovers nested
fixtures, and bounds each worker decode independently. Body and face appearance evidence accepts a valid direct
color or one unambiguous topology-targeted binding to an appearance with a
normalized base color. When both sources exist, their colors must agree;
source-carrier bindings and ambiguous assignments do not satisfy the gate.
An absent direct color and absent topology-targeted binding is a complete
no-assignment state; it does not require a fabricated default color.
Absence of a design-domain loss cannot substitute for neutral feature evaluation
and an exact current-body census comparison.
Higher-level implementation is extra capability until every cumulative gate
passes.
