# STEP Open Items

This document lists the parts of STEP exchange formats that we do not know. The specification `step.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## Top-priority implementation gates

These items are implementation gates for the declared Part 21 envelope. They
are not unresolved Part 21 format rules. The STEP score is L9 only when every
gate below has closure evidence.

### L9-01. Pathological endpoint inference

**Defect.** Endpoint-range recovery previously rebuilt the complete model index
for every edge and used an unbounded practical NURBS search. Large valid inputs
could consume more than three minutes without producing output. This is
unacceptable decode behaviour.

**Current control.** The inference pass reuses one model index. NURBS Newton
search and certified interval search have fixed bounds. Deferred curve and
surface constructors resolve through dependency worklists instead of rescanning
the full population for every chain level. Periodic endpoint sweeps reduce
through `rem_euclid` instead of adding one period per turn. The decode session
charges the worst-case range-inference allowance before the pass. A synthesized
16-point implicit-face regression refuses the 16-point linear allowance at the
`step_implicit_face_plane` operation before plane inference starts. Implicit
plane inference uses the ordered outer-loop winding and a scale-relative
collinearity threshold. The release-build sweep enumerated 4,173 STEP-named
inputs. It decoded and validated 4,054 inputs, returned deterministic
detection or parse errors for 119 inputs, and reached no per-file decode
timeout. The largest accepted input completed in 8.61 seconds and reached
1,339,884 KiB peak resident memory while writing 342,305,575 bytes of CADIR
from 121,000,708 input bytes. This is below the 4 GiB desktop materialization
and retention ceilings. The service profile refused the same input
deterministically at its one-million collection-item ceiling. Endpoint and
termination control therefore satisfy the declared resource envelope.

The evaluator does not clone or sort a complete effective knot partition and
does not clone validated rational weights. It scans validated boundaries, keeps
at most 512 seed-nearest spans for seeded inverse searches, and keeps only the
final 512 valid spans for unseeded containment. Each search examines at most
512 spans.

**Status.** Resolved for the declared STEP Part 21 envelope. The measured
retained-memory ratio remains a performance observation, but it is bounded by
the desktop policy and no longer causes pathological endpoint or decode
termination.

**Closure.** Run the full admitted-file sweep with one timeout per file. Every
file must either complete within the declared limit or return a deterministic
format, syntax, or `ResourceLimit` error. No process may remain CPU-bound after
the timeout. Add a regression fixture for the former quadratic index path and
for bounded NURBS search. The sweep, endpoint-range tests, and implicit-face
work-limit tests satisfy this closure.

### L9-02. Semantic resource accounting

**Defect.** Parser work was bounded before semantic decoding and opaque-record
retention were charged. A parser limit therefore did not bound the complete
decode operation.

**Current control.** Parser tokens, records, parameters, semantic passes,
bounded endpoint inference, and copied opaque-record bytes charge the shared
decode session. Each semantic pass charges the complete parsed source graph
once: records, complex-entity leaves, aggregate members, and nested typed
values. Semantic stages also admit the incremental neutral IR entity
population before the next stage runs, with a final defensive admission after
retention. This prevents a record-only allowance from hiding work proportional
to aggregate depth or decoded output size. The implicit-face plane pass
reserves its complete linear point population before topology decoding. Pcurve
consistency omission indexes coedges once,
so retaining or omitting many failed optional pcurves is linear in the decoded
coedge population. Edition-3 anchor expansion charges every cloned value node
to the same collection-item and work-unit dimensions before materialization;
its independent expansion and depth fuses remain active. The service profile
rejects the large input deterministically with `CollectionItems:
BudgetExceeded` after `used=1,000,000` and `requested=1`. The desktop profile
completes it within 4 GiB, with the 1,339,884 KiB peak reported above. Parser
value nodes, exact interned source names, compact exact identity slots,
streamed byte accounting, explicit record-table backing-storage accounting,
and release of the parsed source graph are active controls.

**Status.** Resolved for the declared desktop and service policies. The policy
accounts parser graph, semantic work, collection growth, output entities, and
retained opaque records. A future tighter memory target would be a new policy
requirement, not an unbounded decode defect.

**Closure.** Exercise desktop and service policies with large, deeply nested,
high-reference, and opaque-heavy inputs. Confirm that the reported dimension,
operation, used amount, and limit are stable. Audit every semantic loop and
retained allocation for an uncovered unbounded path. The large-input policy
probe and the parser, semantic-stage, and opaque-retention accounting tests
satisfy this closure.

### L9-03. Valid IR and complete loss accounting

**Defect.** Decode success does not prove valid IR. Some admitted files still
produce topology or geometric-consistency findings, and the decode report does
not yet prove that every omitted semantic construct has one stable loss.

**Current control.** Geometric-consistency checks use the resolved
document-wide linear uncertainty as their baseline. Edge, vertex, face, and
solved-carrier tolerances widen that baseline when present. A small endpoint
deviation within the declared uncertainty is therefore valid; a larger
deviation remains an error. The STEP reader applies this same contract before
final retention: an optional pcurve that fails the surface-to-edge endpoint
contract is omitted from the neutral coedge, retained with its complete source
closure, and reported as `PcurveOmitted`. The STEP face-bound rule is
source-invalid. The reader rejects an affected sheet member or solid root
before committing neutral topology. It retains the source records as named
opaque data, emits `SourceTopologyInvalid` with source provenance, and emits
`TopologyNotTransferred` for the rejected topology transaction. A disconnected
source shell follows the same transaction rule. The neutral IR therefore does
not contain a face with multiple outer loops or a disconnected shell. Optional
pcurve failures remain omitted from neutral topology and are reported as
`PcurveOmitted`.

**Status.** Resolved for the declared Part 21 envelope. The sweep validated all
4,054 successful decodes with zero validation errors, zero validation
timeouts, and zero unclassified bytes. It produced 190 validation warnings,
all `coedge_pairing`; warnings do not make the IR invalid. The aggregate
machine-readable loss counts were `assembly_placements_not_transferred=42`,
`geometry_not_transferred=1,698`, `pcurve_omitted=308`,
`topology_not_transferred=199`, and `source_topology_invalid=3,711`, plus
explicit diagnostic, noncanonical-syntax, untyped-record, and reference-graph
losses. Source-invalid face and shell losses carry source provenance, and each
rejected topology transaction carries a topology-transfer loss.

**Closure.** For every admitted file, run `cadmpeg validate` on the decoded
artifact. Reconcile typed records, named opaque records, unclassified bytes,
and loss notes. Require zero validation errors and zero unclassified source
bytes. Require one stable source-provenance loss for every rejected invalid
face or shell and one topology-transfer loss for every rejected topology
transaction. The STEP face-bound rule is settled: more than one explicit
`FACE_OUTER_BOUND` is source-invalid and must be rejected before neutral
topology commit, not repaired by reclassification.

### L9-04. Native write and re-decode proof

**Defect.** The writer has synthesized and fixture-level round trips, but this
does not prove semantic write-back for the complete admitted envelope or for
edits to retained documents.

**Current control.** The synthesized unit-cube contract writes and re-decodes
AP203 editions 1 and 2, AP214, and AP242 editions 1 through 3. It checks
deterministic bytes, schema detection, valid IR, a defined geometry/topology
fingerprint, and a translated point edit. Strict mode is checked to refuse
unsupported content before emitting bytes. The fingerprint excludes source
IDs and derived edge parameter ranges, but includes carrier geometry, document
units and tolerances, arena populations, topology cardinalities, orientations,
and coedge parameter data.

The contract runs for every supported target. It writes and re-decodes an
edited document for every target and checks the same fingerprint after the
edit. The strict writer refuses unsupported content before emitting bytes. A
fresh source-less target matrix was also accepted by the independent FreeCAD
STEP importer: AP203 editions 1 and 2, AP214, and AP242 editions 1 through 3
each imported one valid six-face solid. The repeatable check is
[`scripts/verify-step-freecad.py`](../../scripts/verify-step-freecad.py).

**Status.** Resolved for the declared Part 21 target matrix. The overall STEP
score is no longer gated by this item.

**Closure.** Write source-less and edited documents for each supported AP and
version target. Re-decode and validate every result. Compare a defined semantic
fingerprint, verify deterministic output, exercise explicit refusal for
unsupported content, and record independent application acceptance where the
application is available. These checks are now repository tests and the
independent importer script above.

### L9-05. Fuzz and termination proof

**Defect.** Parser tests cover selected malformed structures. The L9 proof
must also cover every reader and writer path with a reproducible bounded fuzz
campaign.

**Current control.** The repository contains separate libFuzzer targets for
lexer, parser, inspection, semantic decode, default writer, custom-header
writer, and degenerate-geometry writer paths. The targets treat panics,
aborts, sanitizer findings, and libFuzzer timeouts as failures; ordinary parse,
validation, and export refusals are expected results.

The current bounded campaign ran `step_lexer`, `step_parser`, `step_reader`,
`step_decode`, `step_writer`, `step_writer_custom`, and
`step_geometry_degenerate` for 1,000 executions per target with a two-second
per-input timeout and a 4 GiB RSS ceiling. All seven targets completed without
a panic, sanitizer finding, timeout, or allocation failure. The checked-in
seed inputs remain the reproducible regression corpus.

**Status.** Resolved for the declared fuzz envelope.

**Closure.** The checked-in targets and bounded campaign satisfy this gate.
Retain minimized synthesized regressions for crashes, hangs, stack growth,
allocation failures, invalid IR, and nondeterministic losses when future
campaigns find them.

## 1. External resources

### ER-01. URI resolution

**Question.** Which base URI and normalization rules apply to each relative URI in a REFERENCE section or a document-reference entity?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." state that a REFERENCE entry binds a local resource name to a resource URI and that a target outside the exchange structure is an external dependency.

**Need.** We must know the rules to identify the external resource that a relative URI selects.

### ER-02. Resource access

**Question.** Which retrieval and authentication procedure applies to each external resource URI?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." identifies URI targets outside the exchange structure as external dependencies. The clear-text exchange structure does not contain an access procedure.

**Need.** We must know the procedure to obtain the selected external resource.

### ER-03. Resource composition

**Question.** How does each external resource combine with the local instance graph?

**Known.** `step.md` §5 "Instance names share one namespace across all DATA sections." through `step.md` §5 "Instance names share one namespace across all DATA sections." define identity and reference resolution inside the DATA sections. `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." define local resource bindings and external dependencies.

**Need.** We must know the composition rule to resolve cross-resource identities and build one product graph.

### ER-04. Resource cache identity

**Question.** Which URI components and resource metadata determine whether two external resource references identify the same cached resource?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." and `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." state that each REFERENCE entry contains a resource URI. The specification gives no cache-identity rule.

**Need.** We must know the identity rule to reuse a retrieved resource without combining different resources.

## 2. AP242 BO-Model sidecars

### BM-01. Sidecar envelope

**Question.** What XML grammar and file relationship identify an AP242 BO-Model sidecar?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" identify an AP242 BO-Model XML sidecar as an encoding that is distinct from the Part 21 clear-text exchange structure.

**Need.** We must know the envelope to detect, parse, and associate the sidecar with its Part 21 exchange structure.

### BM-02. Sidecar composition

**Question.** How do AP242 BO-Model XML identities and values combine with the Part 21 instance graph?

**Known.** `step.md` §5 "Instance names share one namespace across all DATA sections." through `step.md` §5 "Instance names share one namespace across all DATA sections." define identity and reference resolution inside the Part 21 DATA sections. The specification gives no cross-encoding composition rule.

**Need.** We must know the composition rule to build one product graph from the Part 21 exchange structure and its sidecar.

## 3. Containers and other encodings

### CE-01. ZIP container layout

**Question.** Which ZIP entries, names, metadata, and relationships form an edition-3 exchange container?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" identify a ZIP container as distinct from a clear-text Part 21 exchange structure. `step.md` §2 "A clear-text exchange structure uses this outer grammar:" defines the clear-text outer grammar.

**Need.** We must know the layout to locate and identify each exchange resource in the container.

### CE-02. ZIP resource composition

**Question.** How do references between exchange resources in an edition-3 ZIP container resolve?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." define resource names and URIs in a Part 21 REFERENCE section. The specification gives no container-relative resolution rule.

**Need.** We must know the resolution rule to combine the contained resources into one product graph.

### CE-03. Part 28 XML grammar

**Question.** What XML grammar represents an AP203, AP214, or AP242 exchange structure in Part 28?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" define Part 21 clear text and identify Part 28 XML as a distinct encoding.

**Need.** We must know the grammar to parse record boundaries, values, and references from Part 28 XML.

### CE-04. Part 28 graph mapping

**Question.** How does each Part 28 XML construct map to the entity graph and invariants in `step.md`?

**Known.** `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," through `step.md` §5 "Instance names share one namespace across all DATA sections." define Part 21 values, records, identities, and references. The specification gives no Part 28 mapping.

**Need.** We must know the mapping to apply schema decoding to a Part 28 exchange structure.

### CE-05. Part 26 binary grammar

**Question.** What HDF5 layout represents an AP203, AP214, or AP242 exchange structure in Part 26?

**Known.** `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" through `step.md` §1 "Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use" define Part 21 clear text and identify Part 26 binary or HDF5 as a distinct encoding.

**Need.** We must know the layout to parse record boundaries, values, and references from Part 26 data.

### CE-06. Part 26 graph mapping

**Question.** How does each Part 26 HDF5 construct map to the entity graph and invariants in `step.md`?

**Known.** `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," through `step.md` §5 "Instance names share one namespace across all DATA sections." define Part 21 values, records, identities, and references. The specification gives no Part 26 mapping.

**Need.** We must know the mapping to apply schema decoding to a Part 26 exchange structure.

## 4. User-defined names

### UD-01. User-defined entity semantics

**Question.** What entity semantics does each user-defined `!` entity name select?

**Known.** `step.md` §3 "user_name" defines the syntax of a user-defined name. `step.md` §5 "Instance names share one namespace across all DATA sections." through `step.md` §5 "Instance names share one namespace across all DATA sections." require an unknown entity to retain its name, complete spans, and links to other named opaque records.

**Need.** We must know the semantics to transfer a user-defined entity to typed native or neutral records.

### UD-02. User-defined type semantics

**Question.** What value semantics does each user-defined `!` type name select?

**Known.** `step.md` §3 "user_name" defines the syntax of a user-defined name. `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," through `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," define a typed parameter as a name with one parameter.

**Need.** We must know the semantics to decode the wrapped parameter as a typed value.

## 5. Signatures

### SG-01. Signature method selection

**Question.** Which SIGNATURE field identifies the signature method and its parameters?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." define the complete byte boundary of a SIGNATURE section. The specification gives no field grammar for its content.

**Need.** We must know the selection rule to choose the correct signature verification method.

### SG-02. Signed byte sequence

**Question.** Which exact bytes does each signature method authenticate?

**Known.** `step.md` §2 "A clear-text exchange structure uses this outer grammar:" through `step.md` §2 "A clear-text exchange structure uses this outer grammar:" place the optional SIGNATURE section after all DATA sections and before the exchange terminator. `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." define the byte boundary of the SIGNATURE section.

**Need.** We must know the byte sequence to calculate the verification input.

### SG-03. Signature value encoding

**Question.** How does each signature method encode its signature value and verification material in the SIGNATURE section?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." require retention of the complete SIGNATURE byte range. The specification gives no field grammar for the retained content.

**Need.** We must know the encoding to extract the signature value, keys, certificates, and method parameters.

### SG-04. Signature verification result

**Question.** Which validation conditions make each signature valid, invalid, or indeterminate?

**Known.** `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." through `step.md` §7 "REFERENCE entries bind a local resource name to a resource URI." define structural retention only. The specification gives no cryptographic validation conditions.

**Need.** We must know the conditions to report a signature verification result.

## 6. Topology and pcurve decisions

### TP-01. Shared-edge ownership

**Resolved.** A distinct committed topology root is an ownership boundary. A
root key includes the root type, resolved shell identities, and shell
orientations. Aliases with the same key reuse the committed body. When more
than one key exists, every root scopes its shell, face, edge, and vertex
identities by the root instance. The scope decision is computed from the full
root population before construction, so source record order cannot change
identity. The reader does not invent sharing between independent roots.

### TP-02. Seam pcurve selection

**Resolved.** A `SEAM_EDGE` supplies the authoritative pcurve reference. The
reference must be decoded, belong to the edge's seam-curve pcurve list, and
use the coedge face surface. The reader binds that one reference and never
selects a seam branch by endpoint fit or source order. A non-seam oriented edge
uses endpoint continuity only when one same-surface pcurve is selected, or when
tied candidates have the same model-space locus. Distinct unresolved
candidates remain detached and produce a topology loss.

### TP-03. Non-planar pcurve units

**Resolved.** A plane uses `(length, length)`. A cylinder or cone uses
`(angle, length)`. A sphere or torus uses `(angle, angle)`. A NURBS surface
uses native knot-domain values. A linear sweep uses `(directrix, length)` and
a revolution uses `(directrix, angle)`; a transposed revolution swaps the
axes. An offset, subset, or curve-bounded surface inherits its support chart.
Line directrices are length-valued, analytic conic directrices are
angle-valued, and NURBS directrices retain native knot-domain values.
Composite, polyline, and unresolved directrices have no stable unit contract
and are not guessed.

**Current control.** The decoder stores a `PcurveAffineTransform` around the
exact basis carrier when the two axis scales differ. Evaluation, inverse
parameter search, validation, and nested trim/offset handling apply the full
affine map. The writer refuses a transformed pcurve when the target format has
no native carrier that preserves its parameterization and reports
`PcurveOmitted`; it does not emit a geometrically false analytic conic.

### TP-04. Partial solid and tolerant point carriers

**Resolved.** CADIR has no tolerant-point or partial-solid carrier. A
`VERTEX_POINT` without a resolvable `CARTESIAN_POINT`, and every solid root
with a missing mandatory carrier, is rejected atomically. The reader retains
the source records as opaque data and emits a `TopologyNotTransferred` error;
it does not infer coordinates or create a partial body. Salvage applies only
to independent sheet or wire members that are complete.

### TP-05. Implicit face-plane orientation

**Resolved.** A base `FACE` without a surface selects the first explicit
`FACE_OUTER_BOUND`, or the first bound when no outer role exists. Its ordered
ring uses the Newell area normal. The effective bound orientation includes the
enclosing `ORIENTED_FACE` reversal. The reader rejects a non-finite or nearly
collinear ring with a relative threshold of `1e-12` and rejects the topology
root instead of fabricating a plane. The work allowance is linear in the point
count.

### TP-06. Pcurve recursion and normalization

**Resolved.** A `PCURVE` definition must resolve to exactly one item in its
`DEFINITIONAL_REPRESENTATION`. The reader decodes supported 2D line, analytic
conic, polyline, NURBS, trimmed, offset, and affine-replica carriers. A
recursive carrier returns no typed geometry at depth 256 or when an active
record repeats. The active-record guard is released on every return path.
Unsupported or cyclic carriers remain named opaque records and are not
attached to a coedge. Topology that needs such a carrier records a
machine-readable pcurve omission loss.
