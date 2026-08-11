# cadmpeg roadmap

cadmpeg's destination is full semantic interoperability for native CAD: read native models into typed IR, write valid native files, and convert between formats with every loss reported.

Current capability and proof criteria live in [format-support.md](format-support.md). This document states direction, milestones, and contributor entry points.

## Critical path: SolidWorks `.sldprt`

`.sldprt` is the reference proprietary part format for design-history completeness. Open formats already score high on the support ladder. Closing `.sldprt` end to end proves the IR, validation, and native-writing path for history-bearing kernels.

Known gaps: surface-family coverage, sheet classification, multi-shell writing, periodic NURBS writing, feature semantics, and version breadth. Assemblies and external references follow after the part envelope.

## Milestones

### 1. Public `.sldprt` evidence

- Populate the public corpus with contributor-authored CC0 `.sldprt` fixtures across geometry, topology, features, configurations, tessellation, appearance, and versions.
- Publish CAD IR JSON Schema validation for serialized artifacts.
- Define semantic IR equality independent of source-specific or regenerated IDs.
- Define geometric error measures for curves, surfaces, tessellation, and transforms.

### 2. Complete `.sldprt` read

- Close analytic, NURBS, trimmed, offset, blend, and procedural geometry coverage for the declared envelope.
- Recover full body-to-vertex ownership for solid, sheet, multibody, multi-shell, periodic, degenerate, and tolerant cases.
- Link tessellation to exact topology.
- Decode sketches, dimensions, constraints, features, suppression, configurations, and construction history into typed IR.
- Preserve materials, appearances, annotations, metadata, and linked attributes.
- Cover a documented SolidWorks version matrix.
- Leave only intentional named opaque payloads.

### 3. Complete `.sldprt` write and round trip

- Preserve unchanged files byte for byte.
- Regenerate valid native sections after typed IR edits.
- Preserve unsupported native records when edits leave them valid.
- Write supported geometry, topology, tessellation, feature, configuration, appearance, and metadata domains.
- Emit selected SolidWorks target versions with explicit compatibility checks.
- Re-decode generated files against intended IR using semantic identity.
- Verify generated files with independent readers.
- Reject invalid or unrepresentable edits with entity-specific diagnostics.

### 4. Faithful translation

- Complete format-neutral feature, sketch, assembly, annotation, and appearance semantics.
- Map equivalent native constructions between source and target formats.
- Convert unsupported procedural constructions into bounded analytic or NURBS carriers.
- Reconcile units, tolerances, parameter ranges, orientation conventions, and kernel topology rules.
- Preserve persistent identity across format versions and repeated conversions.
- Produce machine-readable reports for mappings, solved equivalents, approximations, repairs, and losses.
- Extend STEP export across every STEP-representable IR domain.
- Add mesh and presentation targets that keep exact geometry available.

### 5. Remaining native formats

Raise every declared envelope to the same read, write, and translation bar:

- Autodesk Fusion `.f3d`
- Autodesk Inventor `.ipt`/`.iam`
- FreeCAD `.FCStd`
- Rhino `.3dm`
- Siemens NX `.prt`
- CATIA V5 `.CATPart`
- Creo Parametric `.prt`
- Bare ASM/ACIS `.sat`/`.smt`/`.smb`/`.sab` streams
- SolidWorks assemblies and related native documents

New formats enter through byte specification, container decode, semantic decode, validation, native write, translation, and hardening. Ladder scores and proof criteria stay in [format-support.md](format-support.md).

### 6. Hardening

- Fuzz container parsers, record decoders, IR parsing, validation, exporters, and native writers.
- Test malformed, truncated, adversarial, and resource-exhausting inputs.
- Run property tests for serialization, unit conversion, transforms, topology invariants, and NURBS evaluation.
- Run round-trip and cross-version suites on real files.
- Compare independent geometric evaluations within declared tolerances.
- Test large parts, deep histories, dense tessellations, and assemblies against memory and runtime budgets.
- Keep output deterministic across machines and repeated runs.
- Version the IR, reports, and public library interfaces with documented migrations.

## Current priorities

1. Build the public `.sldprt` corpus and its manifest verification tooling.
2. Inventory every decoded, opaque, and dropped `.sldprt` record family.
3. Close remaining geometry, sheet classification, multi-shell, periodic NURBS, and feature-semantic gaps.
4. Define semantic IR identity for round trips and file revisions.
5. Add geometric validation and explicit tolerance-delta reporting.
6. Expand semantic writing and re-decode tests across supported SolidWorks versions.
7. Keep decode and export losses aligned with the implementation.

Parallel codec work closes bounded open items without displacing the `.sldprt` path.

## Contributor entry points

- Donate focused CC0 `.sldprt` fixtures through the [corpus process](../corpus/README.md).
- Publish and test the generated CAD IR JSON Schema.
- Extend validators with face-loop orientation, bidirectional ownership, and geometric checks.
- Build corpus manifest verification and coverage reporting.
- Render byte provenance over a hex view for decode inspection.
- Add a GLB exporter with explicit tessellation and presentation losses.
- Resolve a bounded item from a format's `*-open-items.md` file with byte-backed evidence.

See [CONTRIBUTING.md](../CONTRIBUTING.md) for DCO, provenance, testing, and review requirements.
