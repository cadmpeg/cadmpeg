# cadmpeg roadmap

cadmpeg's destination is full semantic interoperability for native CAD. A complete codec reads the entire model, represents its design intent and solved geometry, writes valid native files, and converts to other formats without silent loss.

The first reference format is SolidWorks `.sldprt`. Completing one format end to end will prove the IR, fidelity model, validation strategy, and native writing architecture before the same standard is applied to every other codec.

Current implementation status lives in [format-support.md](format-support.md). This document defines the destination, milestones, and completion gates.

## Definition of complete format support

Native write support alone does not establish complete format support. A complete codec accounts for every applicable semantic domain:

- **Container and versions:** headers, sections, compression, directories, checksums, version gates, and embedded streams.
- **Geometry:** analytic curves and surfaces, NURBS, parameterizations, intersections, offsets, blends, sweeps, and other procedural constructions.
- **Topology:** bodies, lumps, shells, faces, loops, coedges, edges, vertices, orientation, ownership, and non-manifold cases.
- **Tessellation:** display meshes, facet attributes, normals, UVs, levels of detail, and links to exact geometry.
- **Design intent:** sketches, dimensions, constraints, features, dependencies, suppression state, configurations, and construction history.
- **Product structure:** components, instances, placements, external references, assembly constraints, and persistent identities.
- **Presentation and application data:** materials, appearance, annotations, metadata, attributes, and application-specific records such as CAM or toolpath data when present.
- **Native persistence:** unchanged-file preservation, semantic regeneration after edits, target-version selection, and retention of unsupported native data.

A complete decode converts understood data into typed IR. Any remaining source data survives as a named opaque record with byte provenance. No source content disappears without a loss entry.

The [support ladder](format-support.md#support-ladder) scores each codec per envelope; the per-domain profiles behind it identify the failing gates. A ladder score is strict: complete support is L9 proven.

## Fidelity contract

Every conversion classifies each source construct by outcome:

1. **Preserved:** the target retains the native value or payload exactly.
2. **Mapped:** the target receives an equivalent semantic construct.
3. **Solved:** the target lacks the source construction, so cadmpeg emits a mathematical equivalent within a declared error bound.
4. **Lost:** no faithful target representation exists. The report identifies the source construct, reason, and affected entities.

Procedural geometry keeps both the native construction and its solved carrier when available. A target that supports the same construction receives the semantic form. Other targets receive an analytic or NURBS representation whose deviation is measured against the source carrier.

Units and tolerances are part of the fidelity contract. cadmpeg preserves source tolerances, records target tolerances, and reports every widening, healing operation, or topology change required by the target kernel. Conversion must not silently replace a model with a looser approximation.

Format and IR versions follow the same rule. Different source-format revisions decode into a common semantic model. IR readers accept the schema version they implement and reject other versions. Native writers state which target versions they emit and reject constructs those versions cannot represent.

## Reference format: SolidWorks `.sldprt`

`.sldprt` is the first full-fidelity target because the repository already contains:

- CRC-validated container framing and section decoding.
- Analytic and NURBS B-rep carriers.
- Explicit solid and sheet body ownership.
- Derived periodic seams and pcurves.
- Tessellation, appearance, metadata, configurations, and feature history.
- Byte-exact preservation for unchanged files.
- Semantic native regeneration from typed IR.

These capabilities make `.sldprt` the shortest path to proving complete semantic round-trip support. Known gaps remain in surface-family coverage, sheet classification, multi-shell writing, periodic NURBS writing, feature semantics, and version breadth.

The `.sldprt` milestone covers part files. SolidWorks assembly files and their external references belong to the product-structure milestone.

## Milestone 1: Public proof and fidelity measurement

Build the evidence required to measure progress.

- Populate the public corpus with contributor-authored CC0 `.sldprt` fixtures.
- Cover controlled variations in geometry, topology, features, configurations, tessellation, appearance, and format versions.
- Maintain a multidimensional support profile for each codec instead of relying on one highest fidelity level.
- Publish the generated CAD IR JSON Schema and validate serialized artifacts against it.
- Define semantic equality for IR documents independently of source-specific or regenerated IDs.
- Define geometric error measures for curves, surfaces, tessellation, and transformed assemblies.
- Retain unsupported native records needed for faithful rewriting.

This milestone is complete when public fixtures produce deterministic decodes, stable reports, and reproducible support claims.

## Milestone 2: Complete `.sldprt` read

Decode the full semantic content of representative SolidWorks part files.

- Complete analytic, NURBS, trimmed, offset, blend, and procedural geometry coverage.
- Recover full body, lump, shell, face, loop, coedge, edge, and vertex ownership.
- Handle solid, sheet, multibody, multi-shell, periodic, degenerate, and tolerant topology.
- Link tessellation to exact topology and preserve display-specific data.
- Decode sketches, dimensions, constraints, feature dependencies, suppression, configurations, and construction history into typed IR.
- Preserve materials, appearances, annotations, metadata, and linked source attributes.
- Support a documented matrix of SolidWorks file versions.
- Replace format-specific opaque records with typed structures until every remaining opaque payload is intentional and named.

This milestone is complete when each public `.sldprt` fixture decodes without unreported loss and every typed domain passes structural and geometric validation.

## Milestone 3: Complete `.sldprt` write and round trip

Make native writing a semantic operation rather than a container replay.

- Preserve unchanged files byte for byte.
- Regenerate valid native sections after typed IR edits.
- Preserve unsupported native records when edits do not invalidate them.
- Write all supported geometry, topology, tessellation, feature, configuration, appearance, and metadata domains.
- Emit selected SolidWorks target versions with explicit compatibility checks.
- Re-decode every generated file and compare it with the intended IR using semantic identity.
- Verify generated files with independent readers using corpus fixtures.
- Reject invalid or unrepresentable edits with entity-specific diagnostics.

This milestone is complete when unchanged files round-trip byte exactly, edited files round-trip semantically, and every difference is either requested or reported.

## Milestone 4: Faithful translation

Turn the IR into a translation model rather than a collection of decoded records.

- Complete and validate format-neutral feature, sketch, assembly, annotation, and appearance semantics.
- Map equivalent native constructions between source and target formats.
- Convert unsupported procedural constructions into bounded analytic or NURBS representations.
- Reconcile units, absolute tolerances, angular tolerances, parameter ranges, orientation conventions, and kernel-specific topology rules.
- Preserve persistent identity across format versions and repeated conversions.
- Produce machine-readable reports for semantic mappings, solved equivalents, approximations, repairs, and losses.
- Extend STEP export to carry every STEP-representable IR domain.
- Add mesh and presentation targets without reducing exact geometry to the lowest common denominator.

A translation path is complete when converted geometry stays within its declared error bounds, topology remains valid, supported design intent survives, and every unsupported construct appears in the report.

## Milestone 5: Complete the format set

Apply the same read, write, and translation gates to every supported format:

- Autodesk Fusion `.f3d`
- Siemens NX `.prt`
- CATIA V5 `.CATPart`
- Creo Parametric `.prt`
- SolidWorks assemblies and related native documents

Each codec progresses independently across semantic domains. Container inspection or carrier recovery remains useful, but it does not count as complete support. Native writing lands only with round-trip tests and target-version declarations.

New formats enter the project through the same sequence: byte specification, container decode, semantic decode, validation, native write, translation, and hardening.

## Milestone 6: Reliability and hardening

Treat every CAD file as untrusted input and every conversion as a fidelity claim.

The progress gates below apply from the first milestone. This final milestone completes the broader production-hardening work.

- Fuzz container parsers, record decoders, IR parsing, validation, exporters, and native writers.
- Test malformed, truncated, adversarial, and resource-exhausting inputs.
- Run property tests for serialization, unit conversion, transforms, topology invariants, and NURBS evaluation.
- Run round-trip and cross-version suites over representative corpora.
- Compare independent geometric evaluations within declared tolerances.
- Test large parts, deep histories, dense tessellations, and assemblies against memory and runtime budgets.
- Keep output deterministic across machines and repeated runs.
- Version the IR, reports, and public library interfaces with documented migrations.

No format or translation path is reliable until its supported envelope is exercised by a representative corpus of fixtures, mutation tests, and sustained fuzzing.

## Current priorities

Work now follows the `.sldprt` critical path:

1. Build the public `.sldprt` corpus and its manifest verification tooling.
2. Inventory every decoded, opaque, and dropped `.sldprt` record family.
3. Close the remaining geometry, schema-specific sheet classification, multi-shell, periodic NURBS, and feature-semantic gaps.
4. Define semantic IR identity so round trips and file revisions can be compared without source-ID churn.
5. Add geometric validation and explicit tolerance-delta reporting.
6. Expand semantic writing and re-decode tests across supported SolidWorks versions.
7. Keep decode and export losses aligned with the actual implementation.

Parallel codec work should preserve current capabilities and close bounded open items without displacing the `.sldprt` completion path.

## Progress gates

Every completed milestone must satisfy the same project-wide gates:

- A representative corpus of fixtures demonstrates the claimed capability.
- Every source byte is typed, classified as structural, or preserved as an opaque record.
- Every unsupported semantic construct produces a machine-readable loss.
- Decode, validation, write, and conversion results are deterministic.
- Generated files re-decode to the expected semantic IR.
- Geometry and topology satisfy declared tolerance and validity checks.
- Fuzzing covers every parser and writer touched by the milestone.
- [format-support.md](format-support.md) matches the code and tests.

## Contributor entry points

Work that advances these milestones without requiring a complete codec includes:

- Donate focused CC0 `.sldprt` fixtures through the [corpus process](../corpus/README.md).
- Publish and test the generated CAD IR JSON Schema.
- Extend validators with face-loop orientation, bidirectional ownership, and geometric checks.
- Build corpus manifest verification and coverage reporting.
- Render byte provenance over a hex view for decode inspection.
- Add a GLB exporter with explicit tessellation and presentation losses.
- Resolve a bounded item from a format's `*-open-items.md` file with byte-backed evidence.

See [CONTRIBUTING.md](../CONTRIBUTING.md) for DCO, provenance, testing, and review requirements.
