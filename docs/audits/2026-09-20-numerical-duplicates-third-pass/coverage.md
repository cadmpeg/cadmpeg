# Coverage and candidate disposition

The census parsed 1,219 Rust source files and 22,068 non-test function/method bodies across all 21 crate directories. It compared exact token bodies and bodies with consistently renamed identifiers. Numerical screening covered norms, determinants, intersections, transforms, interval/parameter arithmetic, float-to-integer conversions, and checked count/offset operations. Selected candidates received complete-function and caller review. The counts describe screening coverage; they do not mean each function was individually proved correct.

Conventional test modules/directories and test functions were excluded from the production census. Macro-generated functions were not expanded. Small routine accessors, different endian readers, and distinct format-record serializers were not classified as defects merely because their token shapes match.

This pass has 33 ranked entries: 22 demonstrated numerical families and 11 source-reviewed consolidation candidates. No crate exceeds six entries, so the requested 80-per-crate selection limit excludes nothing. Related copies within one crate are grouped. A defect owned by a shared crate is not repeated as a finding in every codec that calls it. Findings resolved by the preceding two audits are excluded; remaining independent copies are named explicitly.

## Reviewed areas by crate

| Crate | Function bodies screened | Candidate review and disposition |
|---|---:|---|
| `cadmpeg` | 390 | Inspect scalar widths and bit casts, query helpers, diff/search windows and checked offset arithmetic. No new ranked issue established. |
| `cadmpeg-asm` | 610 | Cache scans and ambiguity policy, procedural ellipse/support radius paths, binary/text numeric adapters. One duplicate scan family ranked; repaired radius/normalization defects are excluded. |
| `cadmpeg-codec-catia` | 2,023 | Remaining surface Newton search, chart re-fit covariance, source-unit admission, endpoint buckets, spline-line admission, formula functions and duplicate bodies. Two numerical families ranked. |
| `cadmpeg-codec-creo` | 2,321 | Sketch and sweep-profile intersections, repaired quadratic/trim owners, section aggregators, replay consensus and scalar-array helpers. Four numerical families and two consolidation candidates ranked. |
| `cadmpeg-codec-f3d` | 3,210 | Remaining arc-arc and segment predicates, profile distances, bulk-stream sketch decoders and timestamp indexing. Three numerical families and two consolidation candidates ranked. |
| `cadmpeg-codec-freecad` | 878 | Placement normalization, topology similarity gates, endpoint roundoff policy, GUI numeric/color paths, and recursive B-rep census bodies. No new ranked numerical defect established. The census duplication observation is recorded below. |
| `cadmpeg-codec-iges` | 1,105 | Remaining distance copies and B-rep/CSG callers, declared intervals, scalar uncertainty, repaired homogeneous-boundary paths, and relation accessors. One numerical family ranked. |
| `cadmpeg-codec-inventor` | 539 | Native line admission, solved sketch projection, carrier agreement, material scalar conversion and record adapters. One numerical family ranked. |
| `cadmpeg-codec-nx` | 3,041 | Intersection tangent cofactors/fallback, phase lifting, endpoint residuals, JT dequantization, operation visitors and feature admission predicates. Three numerical families and two consolidation candidates ranked. |
| `cadmpeg-codec-rhino` | 1,057 | Plane parameter mapping, knot periodicity and its native-domain guards, legacy object envelopes, mesh f32 gates and existing repaired curve remapping. Two numerical families and one consolidation candidate ranked. |
| `cadmpeg-codec-sat` | 31 | Detection and decode/ASM forwarding. Geometry arithmetic is owned by ASM. No new SAT-local ranked issue established. |
| `cadmpeg-codec-sldprt` | 2,316 | Remaining shared sketch quantizer and equality consumers, duplicate carrier distances, geometry-membership predicates, tessellation distances and configuration hashes. Three numerical families and one consolidation candidate ranked. |
| `cadmpeg-codec-step` | 792 | Similarity gates in both dimensions, transformed carrier/pcurve export, typed scalar readers, validation-property scaling and repaired mesh mass properties. One numerical family and one consolidation candidate ranked. |
| `cadmpeg-container` | 97 | Archive sizes, CFB sector/index arithmetic, chain limits and compression accounting. No new ranked numerical or duplicate issue established. This is not an exhaustive malformed-container/security audit. |
| `cadmpeg-core` | 306 | View conversions, byte assembly, allocated lengths, framing addition proofs, budget arithmetic and distinct-map visitors. No new ranked issue established. |
| `cadmpeg-fuzz` | 203 | Seed/target function bodies and integer conversions. Constant-sized synthetic payload writers are not parsed-count production paths. No new ranked issue established. |
| `cadmpeg-ir` | 2,964 | Remaining residual gates, unary law derivatives, shared math repair boundaries, geometric consistency and recursive law-reference collection. Two numerical families and one consolidation candidate ranked. |
| `cadmpeg-parasolid` | 23 | Prologue/schema-token length and offset arithmetic, detection and classification. No geometry numerical implementation or new ranked duplicate established. |
| `cadmpeg-protein` | 37 | Page framing, typed property carriers, bounded counts, offsets and existing shared material adapters. No new ranked issue established. |
| `cadmpeg-registry` | 67 | Registry/catalog, identification and forwarding candidates. No geometry numerical implementation or new ranked duplicate established. |
| `cadmpeg-test-support` | 58 | Shared harness, binary fixture helpers and duplicate/numeric candidate screening. No new ranked issue established. |

## Other observations and rejected interpretations

- FreeCAD `brep::census_curve` and `census_curve2d` are real parallel recursive family counters. They match over distinct 3D and 2D enums. The duplicated counting policy is an observation, but a new trait or generic geometry hierarchy solely to remove these matches has no established benefit. No behavior disagreement was found. If the curve enums later gain a shared family/child view for another purpose, these counters can share it.
- Creo `DimensionedScalars::fill_tokens` and `CountedScalars::fill_tokens` repeat the same short length-check/unzip/assignment body. This was already recorded in the preceding audit’s coverage notes. It is not a new finding and does not justify an additional storage trait by itself.
- Core `DistinctBTreeMap` and `DistinctHashMap` visitors repeat duplicate-key/error handling, but their collection requirements differ. The behavior is deliberate and covered by local tests. No new map abstraction is proposed just to hide two small loops.
- IR mutable/immutable accessors and the NX fixed-lane wire conversions contain large token-similarity groups. Their borrow and format contracts differ. No shared algorithm was established for these groups.
- SolidWorks face/edge selection rendering has matching enum-dispatch shape. The enums and identifier types differ; the serializer needs to keep those cases explicit. The small relation-entity wrappers already share the actual uniqueness traversal and intentionally use different oriented/unoriented angle semantics.
- CATIA’s endpoint cell casts can saturate for huge coordinates. Neighbor addition saturates, and candidate pairs undergo a robust geometric distance check. Therefore a cell collision alone does not prove a false weld. It can increase candidate work; this pass did not establish a new correctness defect there.
- Several CATIA source-record unit-vector gates square components. A huge or tiny vector is not a unit vector, so rejecting it is correct. Record-specific unit tolerances are not interchangeable normalization policies.
- CATIA’s small-covariance chart cutoff was not ranked independently: at sufficiently small site spacing, the existing 0.002 residual allowance can make both the rotation and reflection admissible. The large-coordinate identity case ranked in the findings avoids that ambiguity.
- Rhino mesh writing checks f32 representability before narrowing. Rejecting a value that the native scalar cannot hold is not the same defect as producing a wrong representable result.
- NX uniform dequantization promotes range arithmetic to f64 before narrowing. Its half-step convention is a format semantic, not a numerical defect established by this pass.
- STEP’s analytic curve-parameter helper projects onto a carrier; absence of a distance test in that projection alone is not proof of an invalid final admission. Validation-property scaling can produce an infinite informational value at extreme units; the reviewed path emits a note rather than claiming a validation pass. Neither was ranked without a stronger contract violation.

## Evidence limits

The 22 Rust probes compile extracted current functions and link cached IR types/evaluator support. Most use actual admitted geometry types. Two use minimal record scaffolds: the SolidWorks entity holds only the geometry field read by its predicate, and the CATIA chart scaffold supplies the same rigid point-map expressions. The failing CATIA chart case exits before constructing a chart. Production arithmetic in the extracted functions is unchanged; restricted visibility is removed only so the standalone harness can compile.

All probes assert the demonstrated incorrect behavior. Their successful exit confirms reproduction, not repair. The source manifest, cached library hash, exact compiler command, complete compiler output, complete run output and exit statuses are retained. Full production-crate type checking and format-file/corpus tests were not run. The source excerpts used for duplicate review are saved separately.

The source-policy checker passed with exit status 0; its complete output is in `evidence/source-policy.log`. Artifact checks confirmed all finding locations and source hashes, unique IDs and per-crate ranks, local report links, and Python syntax. The census script requires the Python `tree_sitter` and `tree_sitter_rust` packages and an output-directory argument; run it from the repository root.

The census and probe sources have per-file SHA-256 identities because other agents edited unrelated files on the same branch during this audit. No other agent’s source changes are part of this report. A source hash is more precise than assuming the entire checkout remained at one commit.

Many P3 examples use extreme finite exponents. Their arithmetic failures are verified; occurrence in real user documents is not measured. Source-confirmed related copies are distinguished from directly probed functions. Zero ranked findings is a result of this bounded review, not proof that no further bugs exist.
