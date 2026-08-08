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

These items are open implementation gates. They are not unresolved Part 21
format rules. The STEP score remains L8 tested until every gate below closes.

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
16-point implicit-face regression refuses the 120 pair comparisons at the
`step_implicit_face_plane` operation before the pair search starts.

**Closure.** Run the full admitted-file sweep with one timeout per file. Every
file must either complete within the declared limit or return a deterministic
`ResourceLimit` error. No process may remain CPU-bound after the timeout. Add a
regression fixture for the former quadratic index path and for bounded NURBS
search.

### L9-02. Semantic resource accounting

**Defect.** Parser work was bounded before semantic decoding and opaque-record
retention were charged. A parser limit therefore did not bound the complete
decode operation.

**Current control.** Parser tokens, records, parameters, semantic passes,
bounded endpoint inference, and copied opaque-record bytes charge the shared
decode session. Each semantic pass charges the complete parsed source graph
once: records, complex-entity leaves, aggregate members, and nested typed
values. It also charges the neutral IR entity count already produced before
the pass. The pairwise point search used for implicit face planes reserves its
complete upper bound before topology decoding. This prevents a record-only
allowance from hiding work proportional to aggregate depth, decoded output
size, or polygon cardinality. Pcurve consistency omission indexes coedges once,
so retaining or omitting many failed optional pcurves is linear in the decoded
coedge population.

**Closure.** Exercise desktop and service policies with large, deeply nested,
high-reference, and opaque-heavy inputs. Confirm that the reported dimension,
operation, used amount, and limit are stable. Audit every semantic loop and
retained allocation for an uncovered unbounded path.

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
closure, and reported as `PcurveOmitted`. The STEP face-bound rule is retained
as a source validity diagnostic. In the current 4,173-file admitted sweep,
4,054 syntactically valid files decoded, with no decode or validation timeout
and no unclassified source bytes. The sweep produced 308 `PcurveOmitted`
losses and no remaining pcurve consistency findings. The remaining 3,697
validation errors are classified as source-invalid topology: 3,693 faces have
more than one explicit outer bound, and four source `OPEN_SHELL` records have
disconnected face components. The decoder retains those records and findings.

**Closure.** For every admitted file, run `cadmpeg validate` on the decoded
artifact. Classify each failure as a source-invalid case with a retained
diagnostic or fix the decoder. Reconcile typed records, named opaque records,
unclassified bytes, and loss notes. Require zero unexplained validation errors
and zero unclassified source bytes. The STEP face-bound rule is settled: more
than one explicit `FACE_OUTER_BOUND` is source-invalid and must remain a
validation finding, not be repaired by reclassification.

### L9-04. Native write and re-decode proof

**Defect.** The writer has synthesized and fixture-level round trips, but this
does not prove semantic write-back for the complete admitted envelope or for
edits to retained documents.

**Closure.** Write source-less and edited documents for each supported AP and
version target. Re-decode and validate every result. Compare a defined semantic
fingerprint, verify deterministic output, exercise explicit refusal for
unsupported content, and record independent application acceptance where the
application is available.

### L9-05. Fuzz and termination proof

**Defect.** Parser tests cover selected malformed structures, but the L9 proof
does not yet cover every reader and writer path with a reproducible bounded fuzz
campaign.

**Closure.** Add parser and writer fuzz targets for the admitted envelope. Run
bounded campaigns with resource policies enabled. Retain minimized synthesized
regressions for crashes, hangs, stack growth, allocation failures, invalid IR,
and nondeterministic losses.

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

**Question.** When one STEP edge or vertex is referenced by multiple independent
shell owners, should CADIR preserve one shared identity, create one occurrence
identity per owner, or reject the conflicting topology?

**Known.** The decoder uses owner-scoped edge and vertex identities when the
source ownership is ambiguous. It keeps the per-occurrence rule and reports an
identity collision when two committed drafts still claim the same destination
identity.

**Need.** We need a standards-valid shared-edge construction and its ownership
semantics before changing the current rule.

### TP-02. Seam pcurve selection

**Question.** Which same-surface pcurve belongs to each seam coedge when a seam
curve carries more than one candidate pcurve?

**Known.** The source curve can carry multiple pcurves for one surface and the
decoder associates one candidate with each coedge use.

**Need.** We need the UV-continuity and orientation rule that selects a
candidate. Serialized occurrence order is not a sufficient rule.

### TP-03. Non-planar pcurve units

**Question.** Which pcurve parameter axes are length-valued for cylinders,
spheres, cones, tori, and other non-planar support surfaces?

**Known.** A plane uses two length-valued axes. A cylinder or cone uses an
angular `u` axis and a length-valued `v` axis. A sphere or torus uses two
angular axes. A NURBS surface uses its native knot-domain parameters. A
transformed surface keeps the parameterization of its basis. The decoder
converts these axes to canonical IR units before it binds a pcurve.

**Need.** Add an affine parameter-space carrier so analytic conics and
`OFFSET_CURVE_2D` pcurves remain exact when the two axis scales differ. Define
the parameter-unit rules for procedural surfaces whose directrix parameter is
not a standard analytic length or angle parameter. Until then, those pcurves
remain opaque and report `PcurveOmitted`; the decoder must not apply one scalar
to both axes.

### TP-04. Partial solid and tolerant point carriers

**Question.** Should CADIR gain a tolerant point carrier or a partial-solid
representation for a solid with one missing mandatory vertex point?

**Known.** Solid roots commit atomically. A missing mandatory point rejects the
complete solid and reports the failed STEP carrier.

**Need.** We need measured loss rates and an IR design before changing the
atomic-solid invariant.

### TP-05. Implicit face-plane orientation

**Question.** Which winding rule defines the normal of an implicit face plane,
and how does it compose with `ORIENTED_FACE` bound reversal?

**Known.** The decoder derives a plane from non-collinear boundary points and
composes explicit face and bound orientation.

**Need.** We need a winding-based rule that uses the outer loop only and a
degeneracy threshold for nearly collinear points.

### TP-06. Pcurve recursion and normalization

**Question.** What normalization and recursion guard rules apply to cyclic
2D curve definitions, 2D `LINE` carriers, and complex `PCURVE` entities?

**Known.** Supported 2D carriers decode as typed pcurves. Unsupported or cyclic
carriers remain opaque.

**Need.** We need fixtures for cycle handling, 2D line normalization, and
complex `PCURVE` support before extending the typed domain.
