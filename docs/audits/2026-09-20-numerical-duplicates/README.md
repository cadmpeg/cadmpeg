# Workspace duplicate-function and numerical audit

Implementation follow-up: all 47 ranked entries are fixed. See [resolutions and post-commit verification](resolutions.md). The original audit below records the pre-fix state.


47 ranked per-crate entries: 36 contain numerical defects and 11 concern duplication only. Some numerical entries also identify duplication. Twelve crates have findings; nine have no new local finding confirmed in this pass. These are per-crate entries, not 47 independent root causes: shared norm arithmetic and two cross-crate consolidation opportunities recur under their affected crates. No crate reached the requested cap of 30; the largest list has nine.

The first fixes should be the F3D half-turn axis, the three Rhino NURBS join/elevation defects, and the F3D/Creo false line-circle intersections. They change geometry without needing values near floating-point limits. Shared IR signed-weight isocurve admission and rational evaluation follow because multiple codecs use them. Range-limit findings are retained below with explicit triggers; they are not evidence of widespread failure in normal CAD files.

Branch: `feat/finish-illegal-states`. Census base: `afe422e6f475514da5f588f282b1a7e42ba8aa9b`. Closing HEAD: `c737eeb728943b5dc244121506901126f1e0822a`. Other work advanced the branch during this read-only audit. The audited function bodies remained unchanged between extraction and the closing fingerprint check. Source fingerprints are in [source-fingerprints.json](./source-fingerprints.json). The investigation made no production or test edits. This directory records its report and evidence for review; the listed findings are not fixes.

## Scope and verification

The scan covered all 20 workspace crates and the excluded cadmpeg-fuzz crate: 22,036 parsed function bodies. Counts include some semantic test/support functions; they are a census, not a claim that every body received a manual proof. I used tokenized exact-body and consistent-identifier comparisons, numerical-pattern searches, manual checks of candidate owners and callers, and targeted Rust reproductions. Accessor pairs, generated conversions, intentional fixture repetition and documented acceptance thresholds were excluded from duplication findings.

The standalone Rust probe compiled and ran with exit status 0. Its output records violations of mathematical invariants; this is not a claim that the crates pass their suites. Most exercised algorithms use extracted current function bodies. A few public IR/ASM operations use already-built cached dependencies; their current implementations were inspected. Error-report adapters in the standalone probe do not change its geometric arithmetic. Arithmetic-only witnesses and paths checked only in source are marked in each entry.

No Cargo check, workspace build, clippy, codec suite, corpus sweep or full encode/decode regression suite was run. This keeps the audit independent of the slow build. A complete guarantee that no other duplicates or numerical bugs exist would require substantially broader execution and is not established here.

Evidence: [Rust probe source](./probes.rs.txt), [complete output](./probes.log), [compiler command](./probe-build.command.txt), [compiler status](./probe-build.exit), [probe status](./probes.exit), [machine-readable findings](./findings.json). The recorded command retains the original temporary paths and cached library hashes. Copy probes.rs.txt to a temporary .rs file and adjust those paths when rerunning it. The saved source is an audit reproduction, not a crate test or a production module.

Ranking: **P1** = wrong geometry on ordinary or small finite inputs; **P2** = other confirmed numerical/admission defects, often requiring unusual scales or integer limits; **P3** = consolidation without a demonstrated numerical failure. Within each crate, entries are ordered by impact and reach. No item is included to fill a quota.

## All-crate coverage

| Crate | Parsed bodies | Numerical entries | Duplication-only entries | Total |
|---|---:|---:|---:|---:|
| cadmpeg | 390 | 0 | 0 | 0 |
| cadmpeg-asm | 612 | 2 | 0 | 2 |
| cadmpeg-core | 306 | 0 | 0 | 0 |
| cadmpeg-container | 97 | 0 | 0 | 0 |
| cadmpeg-ir | 2864 | 7 | 1 | 8 |
| cadmpeg-parasolid | 23 | 0 | 0 | 0 |
| cadmpeg-registry | 67 | 0 | 0 | 0 |
| cadmpeg-test-support | 58 | 0 | 0 | 0 |
| cadmpeg-protein | 28 | 0 | 0 | 0 |
| cadmpeg-codec-f3d | 3221 | 3 | 1 | 4 |
| cadmpeg-codec-inventor | 548 | 1 | 1 | 2 |
| cadmpeg-codec-freecad | 884 | 1 | 1 | 2 |
| cadmpeg-codec-sldprt | 2321 | 3 | 1 | 4 |
| cadmpeg-codec-catia | 2019 | 1 | 0 | 1 |
| cadmpeg-codec-creo | 2341 | 2 | 3 | 5 |
| cadmpeg-codec-nx | 3041 | 2 | 0 | 2 |
| cadmpeg-codec-rhino | 1058 | 8 | 1 | 9 |
| cadmpeg-codec-iges | 1129 | 5 | 0 | 5 |
| cadmpeg-codec-step | 795 | 1 | 2 | 3 |
| cadmpeg-codec-sat | 31 | 0 | 0 | 0 |
| cadmpeg-fuzz | 203 | 0 | 0 | 0 |

## cadmpeg-ir

### 1. [IR-01] P2 — Rational isocurve extraction depends on the common weight factor

Kind: numerical. Evidence status: reproduced.

The extractor rejects nonpositive accumulated weights although NurbsSurface and NurbsCurve admit signed nonzero rational weights. A bilinear surface with all weights -1 is the same surface as all weights +1, but only the latter yields an isocurve. It also accumulates raw basis*weight*coordinate: common weights 1e308 overflow, and minimum-subnormal weights underflow. Preserve signed common-factor invariance and use range-safe weighted sums.

Locations: [crates/cadmpeg-ir/src/eval.rs:2391](../../../crates/cadmpeg-ir/src/eval.rs#L2391).

Verification: probes.log: exact isocurve, basis and wrapping bodies. All surfaces pass NurbsSurface::from_lanes. Weights +1 succeed; -1, 1e308 and minimum subnormal return None. Point evaluation of the -1 surface remains correct.

### 2. [IR-02] P2 — Rational point and derivative evaluation changes under common weight scaling

Kind: numerical. Evidence status: reproduced.

The evaluators accumulate raw weighted coordinates; the tangent then multiplies weighted sums and divides by weight squared. Common scaling must preserve the curve. Poles x=2,4 on [0,1] should give x=3 and dx/dt=2 at t=0.5. Weights 1e308 produce Some(infinity); weights 1e200 or 1e-200 make the tangent None. This is separate from the basis-span defect. Scale homogeneous accumulation and differentiate the normalized quotient without squaring an unscaled total weight.

Locations: [crates/cadmpeg-ir/src/eval.rs:1554](../../../crates/cadmpeg-ir/src/eval.rs#L1554), [crates/cadmpeg-ir/src/eval.rs:2305](../../../crates/cadmpeg-ir/src/eval.rs#L2305), [crates/cadmpeg-ir/src/eval.rs:3000](../../../crates/cadmpeg-ir/src/eval.rs#L3000).

Verification: probes.log: actual extracted curve point/tangent functions; positive common weights 1, 1e200, 1e308, 1e-200, minimum subnormal. Surface point code has the same raw accumulation; surface path was source reviewed, not independently executed.

### 3. [IR-03] P2 — Shared vector lengths and point distances lose representable magnitudes

Kind: numerical. Evidence status: reproduced.

Point3::distance and Vector3::norm square unscaled components. A one-axis separation/vector of 1e200 returns infinity; 1e-200 returns zero. Both correct answers are finite and nonzero. Distance_squared itself need not represent an unrepresentable square; the bug is taking its square root to compute a representable length. Shared downstream geometry checks inherit this.

Locations: [crates/cadmpeg-ir/src/math.rs:49](../../../crates/cadmpeg-ir/src/math.rs#L49), [crates/cadmpeg-ir/src/math.rs:112](../../../crates/cadmpeg-ir/src/math.rs#L112).

Verification: probes.log: public Vector3::norm and Point3::distance on 1e200 and 1e-200, consistent with the source bodies.

### 4. [IR-04] P2 — Affine transform arithmetic rejects representable cancellation results

Kind: numerical. Evidence status: reproduced.

compose, apply_point/apply_vector and inverse translation use plain sums of products. Row [1,1,-1] dotted with [MAX,MAX,MAX] has finite exact result MAX, but the intermediate sum overflows. Point application returns None and composition returns NonFinite; a finite affine inverse similarly fails in its translation. Reuse scaled/exact dot machinery already present for apply_normal. One shared arithmetic family is counted here, rather than every method separately.

Locations: [crates/cadmpeg-ir/src/transform.rs:509](../../../crates/cadmpeg-ir/src/transform.rs#L509), [crates/cadmpeg-ir/src/transform.rs:529](../../../crates/cadmpeg-ir/src/transform.rs#L529), [crates/cadmpeg-ir/src/transform.rs:602](../../../crates/cadmpeg-ir/src/transform.rs#L602).

Verification: probes.log: cached public Transform APIs reproduce point application, composition and inverse-translation failures; all relevant current source methods were read.

### 5. [IR-05] P2 — Point inversion duplicates affine inversion and fails on uniform scale

Kind: numerical + duplicate. Evidence status: reproduced.

inverse_affine_point uses cofactors and a raw determinant instead of the shared affine inverse. A uniform scale of 1e110 has a finite inverse and maps (1e110,1e110,1e110) back to (1,1,1), but this helper returns None because its determinant overflows. It also computes the inverse norm through unscaled squares. Use the owning transform API and a robust norm for the tolerance multiplier.

Locations: [crates/cadmpeg-ir/src/eval.rs:4279](../../../crates/cadmpeg-ir/src/eval.rs#L4279), [crates/cadmpeg-ir/src/transform.rs:602](../../../crates/cadmpeg-ir/src/transform.rs#L602).

Verification: probes.log: exact extracted helper returns None while Transform::try_inverse_affine succeeds on the same input.

### 6. [IR-06] P2 — B-spline basis recurrence overflows on a finite tiny knot span

Kind: numerical. Evidence status: reproduced.

The recurrence computes value/denominator before multiplying by a knot difference. For degree 1, knots [0,0,1e-310,1e-310], poles x=2,4 and the midpoint parameter, the result is Some(NaN,NaN,NaN), although the correct point is (3,0,0). Form bounded knot ratios before multiplying basis values, and require finite evaluator output.

Locations: [crates/cadmpeg-ir/src/eval.rs:1450](../../../crates/cadmpeg-ir/src/eval.rs#L1450).

Verification: probes.log: actual extracted bspline_basis and nurbs_curve_point.

### 7. [IR-07] P2 — Periodic wrapping can return Some(NaN) for finite parameters

Kind: numerical. Evidence status: reproduced.

A finite parameter minus a finite domain start can overflow before rem_euclid. Domain [-1e308,-9e307] and parameter 1e308 produce Some(NaN) despite a finite positive period. Use overflow-safe modular subtraction and reject nonfinite output.

Locations: [crates/cadmpeg-ir/src/eval.rs:2687](../../../crates/cadmpeg-ir/src/eval.rs#L2687).

Verification: probes.log: exact extracted periodic_parameter.

### 8. [IR-08] P3 — NURBS parameter-domain checks duplicate an existing shared helper

Kind: duplicate. Evidence status: source-confirmed.

The local knot-domain implementation duplicates nurbs_pcurve_parameter_domain, including degree/count arithmetic, endpoint selection and finite/increasing checks. Call the existing shared helper directly so evaluation, validation and STEP selection agree.

Locations: [crates/cadmpeg-ir/src/validate/geometry_consistency.rs:957](../../../crates/cadmpeg-ir/src/validate/geometry_consistency.rs#L957), [crates/cadmpeg-ir/src/eval.rs:1591](../../../crates/cadmpeg-ir/src/eval.rs#L1591).

Verification: Token-body census and signature/source comparison; only local endpoint variable names differ.

## cadmpeg-asm

### 1. [ASM-01] P2 — SAT integer tokens lose exact values through f64

Kind: numerical. Evidence status: reproduced.

parse_number stores integral lexical tokens in f64; Cur::long and lexical_token later cast them to i64. 9007199254740993 becomes 9007199254740992; 9223372036854775808 saturates to i64::MAX instead of rejecting. Preserve integer lexemes as exact integers and range-check. Dollar-prefixed references use a separate integer path and are not included in this finding.

Locations: [crates/cadmpeg-asm/src/sat.rs:187](../../../crates/cadmpeg-asm/src/sat.rs#L187), [crates/cadmpeg-asm/src/sat.rs:609](../../../crates/cadmpeg-asm/src/sat.rs#L609), [crates/cadmpeg-asm/src/sat.rs:1314](../../../crates/cadmpeg-asm/src/sat.rs#L1314).

Verification: probes.log: both extracted parse_number+cast and the complete cached cadmpeg_asm::sat::parse API change the integer tokens. The full parser emits Long(9007199254740992) and Long(9223372036854775807).

### 2. [ASM-02] P2 — Kernel direction normalization rejects large finite directions

Kind: numerical. Evidence status: source-confirmed.

unit_vector measures the unscaled norm and then multiplies by a reciprocal. Finite directions at 1e200 or 1e-200 are rejected because their squared norms overflow/underflow. The same routine is duplicated in NX native/vector.rs.

Locations: [crates/cadmpeg-asm/src/nurbs/reader.rs:84](../../../crates/cadmpeg-asm/src/nurbs/reader.rs#L84).

Verification: Its body is the same as the extracted NX unit_vector that returns None for 1e200 and 1e-200 in probes.log; source path reviewed.

## cadmpeg-codec-f3d

### 1. [F3D-01] P1 — Half-turn matrix decomposition can return the wrong rotation axis

Kind: numerical. Evidence status: reproduced.

matrix_axis_angle chooses the signs of y and z only from the x-row off-diagonal entries. A 180-degree rotation about [0,1,-1] has both those entries zero and a negative yz entry. The function returns an axis with y and z the same sign, changing the rotation. Choose the largest diagonal component as the pivot.

Locations: [crates/cadmpeg-codec-f3d/src/design/feature_project.rs:4076](../../../crates/cadmpeg-codec-f3d/src/design/feature_project.rs#L4076).

Verification: probes.log: exact matrix returns axis [0,+1,+1]/sqrt(2), instead of [0,+1,-1]/sqrt(2). project_move uses this result directly.

### 2. [F3D-02] P1 — Small disjoint line and circle are reported as intersecting

Kind: numerical. Evidence status: reproduced.

The discriminant error floor max(1) is not scaled to the geometry. Circle radius 1e-4 at (0,0), line (-1e-4,2e-4) to (1e-4,2e-4): the routine returns (0,2e-4), twice the radius from the center. analytic_segment_intersections accepts this point for profile construction. Use scale-aware arithmetic/error bounds and verify both carrier residuals.

Locations: [crates/cadmpeg-codec-f3d/src/design/geometry.rs:672](../../../crates/cadmpeg-codec-f3d/src/design/geometry.rs#L672).

Verification: probes.log: actual extracted line_arc_intersection_points and angle-membership helper.

### 3. [F3D-03] P2 — Texture distance conversion can introduce nonfinite neutral values

Kind: numerical. Evidence status: source-confirmed.

A finite distance value of 1e308 inches overflows when multiplied by 25.4. distance_property returns the infinity as a successful value; texture mapping and bump depth store it directly. Add finite checked conversion and a loss/refusal for values outside the neutral range. The mathematical result is not representable: the defect is successful propagation, not inability to represent it.

Locations: [crates/cadmpeg-codec-f3d/src/materials.rs:979](../../../crates/cadmpeg-codec-f3d/src/materials.rs#L979).

Verification: Source-to-TextureMap2d/BumpMap flow reviewed; scalar multiplication witness is in the extended probe. Full material decode was not run.

### 4. [F3D-04] P3 — Protein material adaptation is duplicated across F3D and Inventor

Kind: duplicate. Evidence status: source-confirmed.

Shared code includes property suffix lookup, scalar extraction, neutral property naming, texture conversion and material classification. Consolidate the common adapter once while preserving F3D unknown-unit reporting and Inventor caller policy. This is one cross-crate change, listed in each affected crate.

Locations: [crates/cadmpeg-codec-f3d/src/materials.rs:845](../../../crates/cadmpeg-codec-f3d/src/materials.rs#L845), [crates/cadmpeg-codec-f3d/src/materials.rs:934](../../../crates/cadmpeg-codec-f3d/src/materials.rs#L934), [crates/cadmpeg-codec-inventor/src/materials.rs:148](../../../crates/cadmpeg-codec-inventor/src/materials.rs#L148), [crates/cadmpeg-codec-inventor/src/materials.rs:222](../../../crates/cadmpeg-codec-inventor/src/materials.rs#L222).

Verification: Exact duplicate groups plus full texture adapter comparison. Do not replace this with several tiny traits.

## cadmpeg-codec-rhino

### 1. [RHINO-01] P1 — Joining rational segments changes the second segment shape

Kind: numerical. Evidence status: reproduced.

The join shares a control point and discards the second segment first weight without reconciling homogeneous scales. Join a quadratic ending at (0,0) with common weights 2 to a quadratic [(0,0),(0,1),(1,1)] with common weights 1. The second midpoint changes from (0.25,0.75) to (0.2,0.6). Rescale all weights of the next segment to match the shared homogeneous endpoint, or use an exact representation that keeps both endpoint rows.

Locations: [crates/cadmpeg-codec-rhino/src/curves.rs:962](../../../crates/cadmpeg-codec-rhino/src/curves.rs#L962), [crates/cadmpeg-codec-rhino/src/curves.rs:1075](../../../crates/cadmpeg-codec-rhino/src/curves.rs#L1075).

Verification: probes.log: extracted join, degree elevation and basis evaluators; small finite positive weights. Consumers exact_nurbs and C2 polycurve conversion source reviewed.

### 2. [RHINO-02] P1 — Degree elevation includes exterior spans of unclamped curves

Kind: numerical. Evidence status: reproduced.

After inserting active-domain endpoints, elevate_to_degree enumerates every knot interval, including intervals outside the active domain. It does not discard exterior poles. This can extend the domain and change points inside it, even when target degree is unchanged. Restrict the decomposition to the active domain and its correct control slices.

Locations: [crates/cadmpeg-codec-rhino/src/curves.rs:847](../../../crates/cadmpeg-codec-rhino/src/curves.rs#L847), [crates/cadmpeg-codec-rhino/src/curves.rs:897](../../../crates/cadmpeg-codec-rhino/src/curves.rs#L897).

Verification: probes.log: degree-2 knots [-1,-1,0,1,2,2], poles [(0,0),(1,1),(2,0)] at t=0.5 change from (1,0.75) to (0.875,0.875). These knots are exactly the decoder reconstruction of stored knots [-1,0,1,2] (order 3, pole count 3); periodic_knots returns false for that count.

### 3. [RHINO-03] P1 — Degree elevation drops a pole at a full-multiplicity interior knot

Kind: numerical. Evidence status: reproduced.

The span offset span*degree and skip(1) assume every neighboring Bezier span shares an endpoint row. A degree+1 interior multiplicity represents independent rows. Degree 2 knots [0,0,0,1,1,1,2,2,2] with x poles 0..5 lose the final pole 5 and the full multiplicity. Handle disconnected spans explicitly or refuse a join that cannot preserve them.

Locations: [crates/cadmpeg-codec-rhino/src/curves.rs:897](../../../crates/cadmpeg-codec-rhino/src/curves.rs#L897), [crates/cadmpeg-codec-rhino/src/curves.rs:905](../../../crates/cadmpeg-codec-rhino/src/curves.rs#L905).

Verification: probes.log: actual elevate_to_degree at unchanged degree returns only five poles, ending at x=4, and reduces interior multiplicity 3 to 2. The carrier constructor admits the source.

### 4. [RHINO-04] P2 — Domain remapping rejects representable knots due to an overflowing scale factor

Kind: numerical. Evidence status: reproduced.

A source domain [0,1e-200] and target [0,1e200] have finite mapped knots, but computing the scale ratio first produces infinity and then NaN at the first endpoint. Map with bounded relative coordinates and robust interpolation, retaining exact endpoints.

Locations: [crates/cadmpeg-codec-rhino/src/curves.rs:738](../../../crates/cadmpeg-codec-rhino/src/curves.rs#L738).

Verification: probes.log: extracted remap_nurbs_domain returns curve knot remap overflowed for an admitted linear NURBS.

### 5. [RHINO-05] P2 — Joining identical large endpoints overflows their midpoint

Kind: numerical. Evidence status: reproduced.

The join averages endpoints as (a+b)*0.5 even when they are identical. Two finite x=1e308 endpoints produce infinity and edit_control_points refuses the curve. Use f64::midpoint, which also handles opposite signs without overflowing a subtraction.

Locations: [crates/cadmpeg-codec-rhino/src/curves.rs:1043](../../../crates/cadmpeg-codec-rhino/src/curves.rs#L1043).

Verification: probes.log: complete extracted join/elevation path with two linear segments meeting at x=1e308 fails with control_points contains a non-finite point.

### 6. [RHINO-06] P2 — Reversed edge validation reflects parameters through an overflowing sum

Kind: numerical. Evidence status: source-confirmed.

The reversed edge parameter is computed as domain[0]+domain[1]-parameter. Offset domains such as [1e16,1e16+2] shift by one representable step; very large endpoints overflow the sum. Use endpoint-relative reflection. This is the same arithmetic defect in the IGES reversers, in a separate export path.

Locations: [crates/cadmpeg-codec-rhino/src/writer.rs:1389](../../../crates/cadmpeg-codec-rhino/src/writer.rs#L1389).

Verification: Source expression read; shared reflection arithmetic reproduced in probes.log. Complete Rhino edge export not run.

### 7. [RHINO-07] P2 — Quad triangulation selects the longer diagonal when squared lengths overflow

Kind: numerical. Evidence status: reproduced.

The policy compares raw squared diagonal lengths. For vertices [(0,0),(1e200,0),(2e200,2e200),(0,1e200)], diagonal 1--3 is shorter, but both squared lengths become infinity and the <= tie selects 0--2. Double-precision vertices and unit scaling reach this routine. Compare robust lengths or consistently scaled squared lengths. This is an extreme-coordinate mesh quality defect, not a claim of ordinary-file corruption.

Locations: [crates/cadmpeg-codec-rhino/src/mesh.rs:686](../../../crates/cadmpeg-codec-rhino/src/mesh.rs#L686), [crates/cadmpeg-codec-rhino/src/mesh.rs:730](../../../crates/cadmpeg-codec-rhino/src/mesh.rs#L730).

Verification: probes.log: extracted triangulate_faces, unique_face_vertices and distance_squared select triangles [0,1,2] and [0,2,3]. The vertex decode/scaling route was source reviewed.

### 8. [RHINO-08] P2 — Extrusion admission uses the same unstable norm normalization

Kind: numerical. Evidence status: source-confirmed.

A finite extrusion path from (0,0,0) to (1e200,0,0) has representable length, but path_delta.norm() overflows and the reader reports the path invalid. Local normalize and active_miter repeat norm-plus-reciprocal arithmetic. Fix the shared norm root, then retain each local admission contract. This is another affected caller of IR norm, not a separate shared-library root cause.

Locations: [crates/cadmpeg-codec-rhino/src/extrusion.rs:218](../../../crates/cadmpeg-codec-rhino/src/extrusion.rs#L218), [crates/cadmpeg-codec-rhino/src/extrusion.rs:877](../../../crates/cadmpeg-codec-rhino/src/extrusion.rs#L877).

Verification: Source admission and helper read; the exact shared norm was reproduced in probes.log. Full extrusion input not decoded.

### 9. [RHINO-09] P3 — Chunk checksum warning logic is duplicated

Kind: duplicate. Evidence status: source-confirmed.

The two helpers perform the same checksum check and emit the same typed loss code and message. Give this policy one owner in the chunk/checksum layer; keep label and warning sink as arguments.

Locations: [crates/cadmpeg-codec-rhino/src/instances.rs:318](../../../crates/cadmpeg-codec-rhino/src/instances.rs#L318), [crates/cadmpeg-codec-rhino/src/mesh.rs:1445](../../../crates/cadmpeg-codec-rhino/src/mesh.rs#L1445).

Verification: Identifier-normalized duplicate census plus source review.

## cadmpeg-codec-creo

### 1. [CREO-01] P1 — Trim reconstruction invents a line-circle tangent for disjoint carriers

Kind: numerical. Evidence status: reproduced.

The discriminant tolerance uses a scale floored to 1. Circle radius 1e-4 and horizontal segment at y=2e-4 yield the false tangent (0,2e-4). The caller can select this as a trim vertex when no unique shared point is available. Normalize the quadratic or use dimensionally scaled error bounds and residual checks.

Locations: [crates/cadmpeg-codec-creo/src/feature/definitions.rs:3472](../../../crates/cadmpeg-codec-creo/src/feature/definitions.rs#L3472), [crates/cadmpeg-codec-creo/src/feature/definitions.rs:3629](../../../crates/cadmpeg-codec-creo/src/feature/definitions.rs#L3629).

Verification: probes.log: exact extracted trim_line_circle_intersection; caller fallback read.

### 2. [CREO-02] P2 — Direction normalization rejects large finite directions

Kind: numerical. Evidence status: source-confirmed.

normalize_with_length uses the shared unscaled squared norm. [1e200,0,0] is rejected despite a representable magnitude and unit direction. Fix the norm without changing the explicit near-zero acceptance threshold.

Locations: [crates/cadmpeg-codec-creo/src/vecmath.rs:28](../../../crates/cadmpeg-codec-creo/src/vecmath.rs#L28).

Verification: Source inspection plus shared norm reproduction in probes.log; this wrapper was not executed separately.

### 3. [CREO-03] P3 — Four expression entry points repeat one generic parser driver

Kind: duplicate. Evidence status: source-confirmed.

Relation, affine, simultaneous-affine and dimension evaluation repeat parser creation, logical_or, trailing whitespace, full-consumption and finite-value checks. ExpressionParser is already generic over the value algebra. Put the driver on that parser or in one generic function; supply the affine default context at its caller.

Locations: [crates/cadmpeg-codec-creo/src/curve.rs:4803](../../../crates/cadmpeg-codec-creo/src/curve.rs#L4803), [crates/cadmpeg-codec-creo/src/curve.rs:4841](../../../crates/cadmpeg-codec-creo/src/curve.rs#L4841), [crates/cadmpeg-codec-creo/src/curve.rs:4857](../../../crates/cadmpeg-codec-creo/src/curve.rs#L4857), [crates/cadmpeg-codec-creo/src/curve.rs:4874](../../../crates/cadmpeg-codec-creo/src/curve.rs#L4874).

Verification: Three token-identical bodies; the fourth only supplies the default context. Signatures and existing generic parser reviewed.

### 4. [CREO-04] P3 — Legacy numeric field indexing repeats an existing generic implementation

Kind: duplicate. Evidence status: source-confirmed.

The integer and real index builders repeat the generic ValueRecord<K> grouping by (parent,name). Move the generic owner into the legacy module and use it from both consumers. Preserve duplicate rows; do not change ambiguity policy.

Locations: [crates/cadmpeg-codec-creo/src/legacy_feature.rs:118](../../../crates/cadmpeg-codec-creo/src/legacy_feature.rs#L118), [crates/cadmpeg-codec-creo/src/legacy_geometry.rs:654](../../../crates/cadmpeg-codec-creo/src/legacy_geometry.rs#L654), [crates/cadmpeg-codec-creo/src/legacy_geometry.rs:667](../../../crates/cadmpeg-codec-creo/src/legacy_geometry.rs#L667).

Verification: Exact token-body matches and generic/concrete signatures reviewed.

### 5. [CREO-05] P3 — Affected-ID agreement is implemented twice for the same record type

Kind: duplicate. Evidence status: source-confirmed.

agreed_feature_affected_ids and unique_named_affected_ids have identical inputs and conflict/consensus semantics. Put one helper with FeatureAffectedIds and import it directly into history decoding.

Locations: [crates/cadmpeg-codec-creo/src/decode/feature_history/dependencies.rs:284](../../../crates/cadmpeg-codec-creo/src/decode/feature_history/dependencies.rs#L284), [crates/cadmpeg-codec-creo/src/feature/rows.rs:1209](../../../crates/cadmpeg-codec-creo/src/feature/rows.rs#L1209).

Verification: Exact body and signature comparison.

## cadmpeg-codec-sldprt

### 1. [SLDPRT-01] P2 — Configuration real-to-integer alignment accepts 2^63 as i64::MAX

Kind: numerical. Evidence status: reproduced arithmetic.

The cast saturates 2^63 to i64::MAX, then integer as f64 rounds back to 2^63, so the equality check passes and changes the value. The equation evaluator already has the correct exclusive positive bound; reuse that policy.

Locations: [crates/cadmpeg-codec-sldprt/src/history/configuration.rs:1240](../../../crates/cadmpeg-codec-sldprt/src/history/configuration.rs#L1240).

Verification: probes.log: the Rust saturating cast and equality test accept 2^63 and produce i64::MAX.

### 2. [SLDPRT-02] P2 — Hole deduplication merges distinct large-coordinate axes

Kind: numerical. Evidence status: reproduced.

Quantizing by 1e-8 and casting to i64 saturates above roughly 9.22e10 coordinates. Parallel Z axes through x=1e12 and x=2e12 receive identical keys; collecting into HashMap keeps only one placement. Reject out-of-range keys or use a representation that retains scale, and compare geometry before merging. This is an extreme-coordinate case; no ordinary-part incidence is claimed.

Locations: [crates/cadmpeg-codec-sldprt/src/resolved_features/holes.rs:3205](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/holes.rs#L3205), [crates/cadmpeg-codec-sldprt/src/resolved_features/holes.rs:2736](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/holes.rs#L2736), [crates/cadmpeg-codec-sldprt/src/resolved_features/transforms.rs:626](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/transforms.rs#L626).

Verification: probes.log: extracted carrier_placements and canonical_axis return one placement for two admitted finite axes. Related hole_axis_key and generic quantize copies source reviewed.

### 3. [SLDPRT-03] P2 — Duplicated line-angle helpers can return Some(NaN)

Kind: numerical + duplicate. Evidence status: reproduced.

Both angle routines compute raw dot products divided by a product of lengths after calling hypot. Two parallel finite directions (1e200,0) produce infinity/infinity and Some(NaN), not zero. Normalize directions before the dot product, check the result and share the angle policy. The carriers differ, so keep their extraction logic in each owner.

Locations: [crates/cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs:2243](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs#L2243), [crates/cadmpeg-codec-sldprt/src/resolved_features/relation_loci.rs:2618](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/relation_loci.rs#L2618).

Verification: probes.log: actual array-based line_line_angle returns Some(NaN); the SketchEntity-based formula is identical after direction extraction.

### 4. [SLDPRT-04] P3 — Principal-plane frames are duplicated between configuration and geometry

Kind: duplicate. Evidence status: source-confirmed.

Both functions map Front/Top/Right to identical origin, normal and reference axes. Use one codec-owned principal-plane mapping so configuration sketches and resolved reference planes cannot diverge.

Locations: [crates/cadmpeg-codec-sldprt/src/history/configuration.rs:937](../../../crates/cadmpeg-codec-sldprt/src/history/configuration.rs#L937), [crates/cadmpeg-codec-sldprt/src/resolved_features/compact_reference_planes.rs:277](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/compact_reference_planes.rs#L277).

Verification: Exact body match and signatures inspected.

## cadmpeg-codec-freecad

### 1. [FREECAD-01] P2 — Placement quaternions and axes are normalized through unsafe squares

Kind: numerical. Evidence status: reproduced helper; source-confirmed parents.

placement_frame, rotate_vector, and placement_components reject equivalent finite quaternions scaled to 1e200 or 1e-200; tiny nonzero axis-angle axes underflow to zero and are silently replaced by Z. Shared scaling-safe quaternion/axis normalization would fix both design and product paths.

Locations: [crates/cadmpeg-codec-freecad/src/design.rs:1875](../../../crates/cadmpeg-codec-freecad/src/design.rs#L1875), [crates/cadmpeg-codec-freecad/src/design.rs:1972](../../../crates/cadmpeg-codec-freecad/src/design.rs#L1972), [crates/cadmpeg-codec-freecad/src/product.rs:1213](../../../crates/cadmpeg-codec-freecad/src/product.rs#L1213), [crates/cadmpeg-codec-freecad/src/product.rs:1172](../../../crates/cadmpeg-codec-freecad/src/product.rs#L1172).

Verification: probes.log: exact rotate_vector returns the input vector for a half-turn quaternion scaled by 1e200 or 1e-200. placement_frame/placement_components reject these scales before calling it; axis-angle underflow instead substitutes Z. Those parent paths were source traced, not fully executed.

### 2. [FREECAD-02] P2 — Placement decoding is duplicated between design and product paths

Kind: duplicate. Evidence status: source-confirmed.

The same XML placement fields, axis-angle conversion, quaternion normalization, and rotation-matrix formula are maintained twice. Both copies carry the same scale defect. Share the placement conversion, retaining caller-specific error context.

Locations: [crates/cadmpeg-codec-freecad/src/design.rs:1875](../../../crates/cadmpeg-codec-freecad/src/design.rs#L1875), [crates/cadmpeg-codec-freecad/src/product.rs:1114](../../../crates/cadmpeg-codec-freecad/src/product.rs#L1114).

Verification: Read both implementations; near duplication, not byte-identical functions.

## cadmpeg-codec-iges

### 1. [IGES-01] P2 — Decode and writer NURBS reversal shift or reject valid parameter domains

Kind: numerical. Evidence status: reproduced arithmetic.

Both reversers form domain_start+domain_end before subtracting each knot/range endpoint. [1e16,1e16+2] shifts by one ULP; [1e308,1.1e308] is refused because the sum overflows although reflection is finite. Consolidate reflection around endpoint differences, preserving each route error type.

Locations: [crates/cadmpeg-codec-iges/src/entities/composite.rs:488](../../../crates/cadmpeg-codec-iges/src/entities/composite.rs#L488), [crates/cadmpeg-codec-iges/src/writer.rs:3512](../../../crates/cadmpeg-codec-iges/src/writer.rs#L3512).

Verification: probes.log: reflected [1e16,1e16+2] becomes [1e16-2,1e16]. Full reversal bodies and their finite checks reviewed.

### 2. [IGES-02] P2 — Affine wrapper duplicates transform operations and returns unchecked nonfinite points

Kind: numerical + duplicate. Evidence status: reproduced.

Affine stores an IR Transform but reimplements rows, composition, point and vector application. Point/vector methods return raw values without the shared API finite-result check. Row [1,1,-1] applied to [MAX,MAX,MAX] yields infinity despite the exact finite MAX result. Prefer the existing Transform owner and propagate refusal; also fix shared cancellation arithmetic. Do not add another forwarding layer to retain old imports.

Locations: [crates/cadmpeg-codec-iges/src/entities/geometry.rs:621](../../../crates/cadmpeg-codec-iges/src/entities/geometry.rs#L621), [crates/cadmpeg-codec-iges/src/entities/geometry.rs:662](../../../crates/cadmpeg-codec-iges/src/entities/geometry.rs#L662).

Verification: probes.log: source Affine block produces an infinite point, while the cached shared Transform reports None for the same cancellation. Full IGES decode not run.

### 3. [IGES-03] P2 — Similarity orientation rejects an exactly uniform finite scale

Kind: numerical. Evidence status: reproduced.

A diagonal scale of 1e110 is finite, invertible and orientation-preserving. The unscaled determinant overflows, so similarity_orientation returns None and transformed revolution admission reports that the placement cannot preserve the parameterization. Normalize the three columns before orthogonality/orientation checks, retaining the named relative tolerance.

Locations: [crates/cadmpeg-codec-iges/src/entities/surfaces.rs:93](../../../crates/cadmpeg-codec-iges/src/entities/surfaces.rs#L93), [crates/cadmpeg-codec-iges/src/entities/surfaces.rs:1909](../../../crates/cadmpeg-codec-iges/src/entities/surfaces.rs#L1909).

Verification: probes.log: extracted similarity_orientation plus the source Affine wrapper return None for this matrix. The transform constructor admits it and the revolution caller was traced.

### 4. [IGES-04] P2 — Direction helpers inherit unstable norm arithmetic

Kind: numerical + duplicate. Evidence status: source-confirmed.

Annotation normalized and writer unit compute a raw norm then multiply by its reciprocal, repeating the ASM/NX pattern. A finite direction (1e200,0,0) is refused despite its representable magnitude. Fix the shared norm/direction arithmetic while preserving the writer epsilon gate and annotation nonzero gate; these different admission policies should not be erased by consolidation.

Locations: [crates/cadmpeg-codec-iges/src/entities/annotation.rs:44](../../../crates/cadmpeg-codec-iges/src/entities/annotation.rs#L44), [crates/cadmpeg-codec-iges/src/writer.rs:6296](../../../crates/cadmpeg-codec-iges/src/writer.rs#L6296).

Verification: Source review and shared norm/NX exact-body reproduction. The annotation and IGES writer callers were not executed independently.

### 5. [IGES-05] P2 — Binary real decoding rejects a representable value near the f64 limit

Kind: numerical. Evidence status: reproduced.

read_real computes fraction*2^exponent. With 12 exponent bits, 51 fraction bits, biased exponent 3072 and fraction field zero, the exact value is 0.5*2^1024=2^1023. The intermediate power overflows and the valid finite value is rejected. Combine mantissa and exponent without first constructing an unrepresentable power. This affects a nondefault Binary primitive-width declaration, not ordinary ASCII IGES.

Locations: [crates/cadmpeg-codec-iges/src/binary.rs:203](../../../crates/cadmpeg-codec-iges/src/binary.rs#L203).

Verification: probes.log: actual BitReader read_bits/align_zero/read_real on the encoded 64-bit primitive returns Malformed; 2^1023 is finite.

## cadmpeg-codec-step

### 1. [STEP-01] P2 — Pcurve export can silently write a zero vector for a finite direction

Kind: numerical. Evidence status: reproduced arithmetic.

direction2 and the Line pcurve writer square finite direction components to get magnitude. Direction [1e200,0] yields infinite magnitude and zero unit coordinates; real() serializes the infinite magnitude as 0. The resulting zero STEP VECTOR changes the curve instead of refusing it.

Locations: [crates/cadmpeg-codec-step/src/geometry.rs:128](../../../crates/cadmpeg-codec-step/src/geometry.rs#L128), [crates/cadmpeg-codec-step/src/geometry.rs:171](../../../crates/cadmpeg-codec-step/src/geometry.rs#L171), [crates/cadmpeg-codec-step/src/writer.rs:88](../../../crates/cadmpeg-codec-step/src/writer.rs#L88).

Verification: probes.log confirms LinePcurve::try_new admits [1e200,0] and the copied magnitude arithmetic yields [0,0] plus infinity. direction2, pcurve and real source flow reviewed; no full STEP export was run.

### 2. [STEP-02] P3 — NURBS parameter-domain checks duplicate an existing shared helper

Kind: duplicate. Evidence status: source-confirmed.

The local knot-domain implementation duplicates nurbs_pcurve_parameter_domain, including degree/count arithmetic, endpoint selection and finite/increasing checks. Call the existing shared helper directly so evaluation, validation and STEP selection agree.

Locations: [crates/cadmpeg-codec-step/src/reader/topology.rs:4214](../../../crates/cadmpeg-codec-step/src/reader/topology.rs#L4214), [crates/cadmpeg-ir/src/eval.rs:1591](../../../crates/cadmpeg-ir/src/eval.rs#L1591).

Verification: Token-body census and signature/source comparison; only local endpoint variable names differ.

### 3. [STEP-03] P3 — Surface-curve wrapper resolution is duplicated

Kind: duplicate. Evidence status: source-confirmed.

curve_carrier_record and curve_carrier_step repeat the same Exchange lookup and SURFACE_CURVE/SEAM_CURVE/INTERSECTION_CURVE unwrapping. Use one reader helper with the source-record owner, imported directly.

Locations: [crates/cadmpeg-codec-step/src/reader/geometry.rs:5153](../../../crates/cadmpeg-codec-step/src/reader/geometry.rs#L5153), [crates/cadmpeg-codec-step/src/reader/topology.rs:3379](../../../crates/cadmpeg-codec-step/src/reader/topology.rs#L3379).

Verification: Identifier-normalized body and signature comparison.

## cadmpeg-codec-nx

### 1. [NX-01] P2 — Surface least-squares correction rejects orthogonal derivatives with different scales

Kind: numerical. Evidence status: reproduced.

The Gram determinant is compared with EPSILON*max(du_squared,dv_squared)^2. Perpendicular derivatives du=(1,0,0), dv=(0,1e-9,0) and residual=(0,1e-9,0) are rejected even though the correction is exactly (0,1). Surface parameter units can produce this scaling without geometrical degeneracy. Normalize columns before the rank test and solve in those scaled coordinates. The same raw products can also overflow at large scales.

Locations: [crates/cadmpeg-codec-nx/src/decode/offset.rs:2162](../../../crates/cadmpeg-codec-nx/src/decode/offset.rs#L2162).

Verification: probes.log: actual least_squares_step returns None. Projection and intersection-tangent callers were reviewed; no full intersection decode run.

### 2. [NX-02] P2 — Native direction normalization rejects large and tiny finite directions

Kind: numerical. Evidence status: reproduced.

The finite nonzero direction contract is implemented with an unscaled squared norm and reciprocal multiplication. [1e200,0,0] and [1e-200,0,0] are refused. It duplicates cadmpeg-asm::nurbs::reader::unit_vector.

Locations: [crates/cadmpeg-codec-nx/src/native/vector.rs:14](../../../crates/cadmpeg-codec-nx/src/native/vector.rs#L14).

Verification: probes.log: exact unit_vector body returns None for both 1e200 and 1e-200.

## cadmpeg-codec-inventor

### 1. [INVENTOR-01] P2 — Texture distance conversion can introduce nonfinite neutral values

Kind: numerical. Evidence status: source-confirmed.

A finite distance value of 1e308 inches overflows when multiplied by 25.4. distance_property returns the infinity as a successful value; texture mapping and bump depth store it directly. Add finite checked conversion and a loss/refusal for values outside the neutral range. The mathematical result is not representable: the defect is successful propagation, not inability to represent it.

Locations: [crates/cadmpeg-codec-inventor/src/materials.rs:255](../../../crates/cadmpeg-codec-inventor/src/materials.rs#L255).

Verification: Source-to-TextureMap2d/BumpMap flow reviewed; scalar multiplication witness is in the extended probe. Full material decode was not run.

### 2. [INVENTOR-02] P3 — Protein material adaptation is duplicated across F3D and Inventor

Kind: duplicate. Evidence status: source-confirmed.

Shared code includes property suffix lookup, scalar extraction, neutral property naming, texture conversion and material classification. Consolidate the common adapter once while preserving F3D unknown-unit reporting and Inventor caller policy. This is one cross-crate change, listed in each affected crate.

Locations: [crates/cadmpeg-codec-f3d/src/materials.rs:845](../../../crates/cadmpeg-codec-f3d/src/materials.rs#L845), [crates/cadmpeg-codec-f3d/src/materials.rs:934](../../../crates/cadmpeg-codec-f3d/src/materials.rs#L934), [crates/cadmpeg-codec-inventor/src/materials.rs:148](../../../crates/cadmpeg-codec-inventor/src/materials.rs#L148), [crates/cadmpeg-codec-inventor/src/materials.rs:222](../../../crates/cadmpeg-codec-inventor/src/materials.rs#L222).

Verification: Exact duplicate groups plus full texture adapter comparison. Do not replace this with several tiny traits.

## cadmpeg-codec-catia

### 1. [CATIA-01] P2 — Isocurve scaling erases mixed-magnitude weights and contributions

Kind: numerical. Evidence status: reproduced.

The recent common-scale repair divides every active weight by the global maximum before accounting for coordinates. Weights 1e308 and 1e-308 can become 1 and 0. A valid isocurve is refused if an entire output column is erased; a pole with weight 1e-308 and x=1e308 can lose a finite contribution, returning x=0 instead of about 1e-308. Preserve exponent range through the weighted sums. This is a new edge case in the recently changed implementation, not the previously fixed common-factor case.

Locations: [crates/cadmpeg-codec-catia/src/nurbs.rs:842](../../../crates/cadmpeg-codec-catia/src/nurbs.rs#L842), [crates/cadmpeg-codec-catia/src/nurbs.rs:876](../../../crates/cadmpeg-codec-catia/src/nurbs.rs#L876).

Verification: probes.log: actual extracted isocurve and basis functions, with reporting-only adapters; both surfaces admitted by NurbsSurface::from_lanes.

## Crates with no new local finding confirmed

**cadmpeg** — Reviewed numeric inspection/scalar conversion, buffer windows, query counters and the duplicate census. No new local defect confirmed.

**cadmpeg-core** — Reviewed View/bounded reads, resource accounting and allocation/span arithmetic. Saturating budgets and guarded additions are intentional; no new local defect confirmed.

**cadmpeg-container** — Reviewed ZIP/CFB size, sector, chain and ownership arithmetic plus duplicate candidates. No new local defect confirmed.

**cadmpeg-parasolid** — Reviewed schema/token/header framing and arithmetic candidates. No new local defect confirmed.

**cadmpeg-registry** — Reviewed registry/dispatch and dialect metadata plus duplicate candidates. No new local defect confirmed.

**cadmpeg-protein** — Reviewed schema framing, scalar reads and property decoding. The texture unit-conversion defect belongs to its F3D/Inventor consumers; no new local defect confirmed.

**cadmpeg-test-support** — Reviewed fixture byte/container helpers and golden/round-trip harness candidates. Trusted fixture writers are not parser bugs merely because they use ordinary indexing. No new local defect confirmed.

**cadmpeg-codec-sat** — Reviewed the codec facade and its ASM delegation. No separate codec-local defect confirmed. The shared cadmpeg-asm SAT integer and normalization findings affect this format.

**cadmpeg-fuzz** — Reviewed the excluded crate census, seed builders and numeric candidates. Intentional fixture repetition is not counted as production duplication. No new local defect confirmed.

## Observations with limited demonstrated impact

**NX nullspace scaling:** [null_vector_3x4](../../../crates/cadmpeg-codec-nx/src/decode/offset.rs#L2031) returns a unit null vector for a well-conditioned matrix at scale 1 and None at scale 1e-5. Its absolute minor-norm threshold causes the discrepancy. However, [intersection_parameter_tangent](../../../crates/cadmpeg-codec-nx/src/decode/offset.rs#L1906) has a least-squares fallback that can recover this case. The local scale dependence is reproduced, but no additional end-to-end failure is claimed or counted. The separate least_squares_step finding above has its own explicit trigger.

**Other shared-norm occurrences:** CATIA standard-support checks, SLDPRT intersection/tessellation distances and IGES evaluation distances contain additional raw squared-length calculations. They share the established range-limit failure mechanism; not every occurrence is a separate ranked bug. A shared norm fix will not automatically repair manually inlined copies. Their full decode paths were not executed in this audit.

**Rhino tolerance conversion:** absolute_tolerance_millimeters can overflow its multiplication, but build_ir routes the result through admitted_tolerance, which records a default repair. This was not promoted to a silent-invalid-tolerance finding.

**Duplication scope:** short same-shape field accessors and wire conversions often express different domain types. The report does not recommend adding generic traits solely to reduce their line count. The selected consolidation entries share a policy or algorithm that can use an existing owner.

## Suggested implementation order

1. Add trigger fixtures and fix F3D rotation, Rhino rational joins and span extraction, and the two false-intersection algorithms. These are independent codec-owned changes.
2. Repair shared IR arithmetic and signed-weight handling, then update affected normalizers and manual copies while preserving their explicit acceptance tolerances. Keep the CATIA mixed-scale regression in the same numerical review.
3. Fix exact integer conversion, saturated hole keys, STEP nonfinite export, IGES/Rhino range handling and checked material conversion.
4. Consolidate the listed policy duplicates in separate behavior-preserving commits. Update callers to the owning module directly.

For implementation, retain the requested workflow: commit with --no-verify and its reason in the commit body before Cargo verification, then run scoped quiet cargo check --tests and targeted tests after the commit. Fix verification failures in follow-up commits on the same branch.
