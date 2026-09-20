# Fourth numerical and duplication audit — 2026-09-20

30 ranked findings: 27 numerical issues and 3 pure duplication groups, across 10 crates. All 21 crates were screened. The maximum is 10 findings in cadmpeg-ir; the requested cap is 30 per crate. The other 11 crates have no new established finding in this pass, not a proof of correctness.

The first three audit repair lists were excluded. Remaining copies or different call paths of an earlier bug family are identified explicitly below. No production code was changed in this audit.

The highest-priority issue is IR4-10: at ordinary finite values, shared parabola coordinate scaling changes the curve parameterization. STEP and Creo use this scaling method. IGES4-01 is a separate dormant evaluator discrepancy; its current decode factory constructs NURBS. Other findings include false geometric admission, scale-dependent intersection decisions, lost finite values/derivatives, and small wrappers that duplicate an existing owner.

## Evidence and limits

- A fresh Rust AST census screened 22,005 production functions in 1,221 files, with no parse errors. Exact-token and identifier-normalized body groups were reviewed as candidates, not counted automatically as defects.
- 26 lightweight Rust witness tests reproduce numerical failures. The test assertions confirm the current wrong result; their passing status does not mean production is correct. Complete compiler and execution logs, exit statuses, source hashes and cached-library hashes are in [evidence](evidence/).
- Most probes extract the current production functions unchanged. Large dispatchers use exact expression probes where noted. CATIA probes have explicitly unused-branch scaffolds. Public IR calls use an existing cached rlib; its hash is recorded, and the implicated current source was also reviewed. These are focused arithmetic/owner witnesses, not full codec round trips.
- No Cargo build, workspace test run, corpus decode or golden regeneration was performed. Runtime prevalence in project files is not measured. Most extreme-range witnesses use finite values near f64 limits; the report does not claim those are common CAD inputs.
- Independent test-placement, cleanup and merge commits landed during the audit. The census records its scan inputs; numerical probe and finding manifests record the subsequently reviewed source. Finding-site hashes were checked at report generation. See [coverage](coverage.md), [machine-readable findings](findings.json), and [revision metadata](evidence/revision.json).

Priorities: P1 is an ordinary-value geometry error with a direct consumer; P2 is a reproduced behavior defect under the stated trigger; P3 is maintenance-only duplication. Rank is local to each crate. The findings give the defect; implementation scope remains a separate decision.

## Findings by crate

### cadmpeg-asm

#### ASM4-01 — P2 — Transform patching can silently replace a finite translation with zero

Locations: `crates/cadmpeg-asm/src/edit.rs:940`.

header_scale=1e308 and translation=1e308 are both admitted finite values. Multiplying the header scale by LEN_TO_MM=10 overflows; the patch writes 0 instead of the representable native translation 0.1. The decode expression with native 0.1 yields the finite model translation.

Repair: Evaluate the conversion as a range-safe multiply/divide and check the final value before writing.

Evidence: `asm::finite_translation_patch`; exact conversion expression; byte patch dispatch source-reviewed.

### cadmpeg-codec-catia

#### CATIA4-01 — P2 — Analytic surface membership cancels the radial component

Locations: `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:7578`.

On a radius-one Z cylinder, (1,0,0) is accepted but the equally valid point (1,0,1e8) is rejected. Computing |offset|^2-axial^2 loses the unit radial term. Cone and torus membership use the same subtraction. This occurs before squared-norm overflow.

Repair: Form the transverse vector, then measure it with the shared robust norm; keep each carrier residual in length units.

Evidence: `catia::axial_cancellation`; extracted analytic function with actual admitted IR cylinder; unused NURBS branch stubbed.

#### CATIA4-02 — P2 — Circle/sphere carrier agreement can accept disjoint geometry after overflow

Locations: `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:9502`, `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:9587`.

A unit circle centered at (1e200,0,0) is not a section of a unit sphere at the origin, but circle_axis_from_carrier returns an axis. Its squared-distance sum is infinite and close_squared accepts infinity <= infinity. The torus section branch uses the same comparison.

Repair: Compare normalized lengths or use scaled squared sums, and reject nonfinite comparison operands.

Evidence: `catia_axis::disjoint_circle_and_sphere`; extracted carrier function with actual admitted IR sphere; normalizer scaffold uses shared unit_nonzero.

### cadmpeg-codec-creo

#### CREO4-01 — P2 — Profile segment intersection misses a small perpendicular crossing

Locations: `crates/cadmpeg-codec-creo/src/decode/sweep/profiles.rs:565`.

Segments [(-1e-5,0),(1e-5,0)] and [(0,-1e-5),(0,1e-5)] cross at the origin. At tolerance 1e-9 the helper returns false. Multiplying tolerance by max(length,1) treats their signed areas as zero, while none of the endpoints passes the other segment box test. The same geometry at unit scale succeeds.

Repair: Use an orientation predicate with dimensionally correct distance tolerance, preserving proper crossings independently of endpoint tests.

Evidence: `creo::small_crossing_segments`; extracted function.

#### CREO4-02 — P3 — Four remaining section collectors repeat traversal, relocation and sorting

Locations: `crates/cadmpeg-codec-creo/src/container.rs:1327`, `crates/cadmpeg-codec-creo/src/container.rs:1439`, `crates/cadmpeg-codec-creo/src/container.rs:1459`, `crates/cadmpeg-codec-creo/src/container.rs:1499`.

These four functions repeat collect/decode/relocate/sort, changing the record decoder and the two relocated offsets. The three Xsections collectors repaired in CREO3-05 are different call sites. Its current helper also filters Xsections, so these all-section callers cannot simply call it unchanged.

Repair: Use one collector over a selected section iterator, with record-specific relocation at the caller. Preserve the Xsections filter where required.

Evidence: AST shape match and complete function/source review.

### cadmpeg-codec-f3d

#### F3D4-01 — P2 — Spatial dimension matching repeats the infinity-equals-infinity acceptance bug

Locations: `crates/cadmpeg-codec-f3d/src/design/dimensions.rs:3334`.

Two spatial points separated by 1e200 are accepted for expected length 1. The raw squared norm overflows; expected.is_finite does not protect the measured distance or the resulting infinite tolerance.

Repair: Use Point3::distance and require a finite measured distance before applying tolerance.

Evidence: `f3d::incorrect_spatial_dimension`; extracted function.

#### F3D4-02 — P2 — Short-line membership uses a scale-dependent distance tolerance

Locations: `crates/cadmpeg-codec-f3d/src/design/dimensions.rs:5345`.

The line from (0,0) to (1e-6,0) accepts (0.5e-6,1e-4), although the declared comparison epsilon is 1e-9. The cross product is compared to epsilon*(1+length), giving a short line an effective transverse tolerance near epsilon/length. ReferenceLine repeats this policy.

Repair: Compute a geometric perpendicular distance with independently normalized directions; retain the explicit segment-parameter bounds.

Evidence: `f3d::short_line_membership`; extracted function.

#### F3D4-03 — P2 — Conic point-membership comparison accepts an infinite equation residual

Locations: `crates/cadmpeg-codec-f3d/src/design/dimensions.rs:5345`.

An origin-centered ellipse with radii 1 and 0.5 accepts (1e200,0). The squared normalized coordinate becomes infinity, then the local close closure accepts infinity <= infinity. The same closure is used by other analytic branches.

Repair: Require finite comparison operands and evaluate normalized conic residuals without unchecked squared overflow.

Evidence: `f3d::remote_ellipse_membership`; extracted function.

#### F3D4-04 — P2 — Mesh normal transformation overflows for an admitted invertible matrix

Locations: `crates/cadmpeg-codec-f3d/src/design/decode/mesh.rs:153`, `crates/cadmpeg-codec-f3d/src/records/mesh.rs:1010`.

MeshAffineTransform admits diag(1e200,1e200,1e-200): its determinant is finite and nonzero. Transforming a Z normal then fails because the Z cofactor multiplies 1e200 by 1e200 before normalization. The correct oriented unit normal is still Z.

Repair: Use a scaled cofactor or the shared robust normal transform plus a reliable orientation sign.

Evidence: `f3d_mesh::valid_anisotropic_normal`; extracted production constructor, accessors and normal method.

### cadmpeg-codec-freecad

#### FREECAD4-01 — P2 — Implicit extrusion length overflows although Dir has a finite norm

Locations: `crates/cadmpeg-codec-freecad/src/design.rs:3854`.

For a custom Part::Extrusion with Dir=(1e200,0,0) and both explicit lengths zero, raw_direction.unit succeeds but the squared magnitude is infinity. The fallback length filter discards a representable length and the feature projection returns None.

Repair: Use Vector3::norm for the Dir length, preserving the existing zero-length and finite-result policy.

Evidence: `expressions::freecad_default_extrusion_length`; exact magnitude expression plus actual unit normalization; property dispatch source-reviewed.

### cadmpeg-codec-iges

#### IGES4-01 — P2 — Private pcurve evaluation uses the wrong parabola parameterization

Locations: `crates/cadmpeg-codec-iges/src/entities/evaluation.rs:46`.

For a neutral ParabolaPcurve at the origin with unit axes, focal distance 2 and t=1, IGES returns (2,4), whereas the IR contract/evaluator returns (0.125,1). IGES evaluates the 3D-style (f*t^2,2*f*t) formula instead of the pcurve contract (t^2/(4*f),t). B-rep endpoint and trimming checks call this evaluator, but their current pcurve_geometry factory always constructs NURBS. The parabola arm is therefore a demonstrated dormant inconsistency, not a reproduced current-file decode failure.

Repair: Remove the duplicate carrier arithmetic and use the shared pcurve evaluator while preserving any explicit supported-family admission policy.

Evidence: `iges_eval::parabola_parameterization`; extracted function.

#### IGES4-02 — P2 — Private rational curve evaluators retain overflowing weighted accumulation

Locations: `crates/cadmpeg-codec-iges/src/entities/evaluation.rs:8`, `crates/cadmpeg-codec-iges/src/entities/evaluation.rs:46`, `crates/cadmpeg-codec-iges/src/entities/evaluation.rs:182`.

The local pcurve evaluator returns Some((infinity,0)) for degree-one poles (1e200,0),(2e200,0) with equal weights 1e200; the finite midpoint is (1.5e200,0). The 3D curve branch duplicates the same raw accumulation. The local basis recursion is another copy of the shared owner. IR4-04 identifies the corresponding remaining 2D shared defect, so delegation alone needs that repair.

Repair: Consolidate carrier evaluation into IR and remove the private basis/weighted loops; preserve supported-family policy separately if it is required.

Evidence: `iges_eval::weighted_nurbs_overflow`; extracted pcurve/basis and source review of the 3D copy; no full IGES decode.

#### IGES4-03 — P2 — Conic classification depends on the arbitrary coefficient multiplier

Locations: `crates/cadmpeg-codec-iges/src/entities/conics.rs:235`.

Multiplying every coefficient of x^2+y^2-1=0 by 1e-200 leaves the unit circle unchanged. The decoder tests A*C for its sign; the product underflows to zero and neither ellipse nor hyperbola classification succeeds.

Repair: Compare nonzero coefficient signs without multiplying them, and normalize the remaining coefficient ratios/classification tests.

Evidence: `more_expressions::conic_coefficient_scale`; exact classification expressions; full projection guards source-reviewed.

#### IGES4-04 — P2 — Conic writing can emit zero or infinite principal coefficients

Locations: `crates/cadmpeg-codec-iges/src/writer.rs:5847`.

The ellipse and hyperbola writers admit positive finite radii, then square them before inversion. Radius 1e200 produces coefficient 0; radius 1e-200 produces infinity. With the constant fixed at -1, the large equal-radius ellipse becomes an equation with no points. number formats the result without a finite check.

Repair: Choose a representable common scaling for conic coefficients, or refuse an unrepresentable coefficient set before emitting a record.

Evidence: `expressions::iges_conic_coefficients`; exact coefficient expressions; admission and serialization source-reviewed.

### cadmpeg-codec-nx

#### NX4-01 — P2 — Damped 4x4 solver squares coefficients before scaling them

Locations: `crates/cadmpeg-codec-nx/src/decode/offset.rs:2137`.

The consistent rank-three system diag(a,a,a,0)*x=(a,a,a,0) has the finite minimum-norm solution (1,1,1,0). The solver succeeds at a=1 and returns None at a=1e-200 and 1e200 because it forms raw normal equations before computing column scales. This is the damped 4x4 fallback, distinct from the previously repaired 2-column surface solver and nullspace tangent.

Repair: Scale the input columns/rows before normal products, and compare residual norms without premature squaring.

Evidence: `nx::scaled_rank_deficient_solve`; extracted function.

#### NX4-02 — P3 — Offset point-distance wrapper repeats the owning Point3 method

Locations: `crates/cadmpeg-codec-nx/src/decode/offset.rs:2205`.

This helper repeats the same three chained hypot operations as Point3::distance and adds no admission rule. Other intersection distance copies were removed in NX3-03; this offset owner remains.

Repair: Replace calls with Point3::distance and remove the redundant helper, updating imports at their owning modules.

Evidence: complete function and caller inventory reviewed.

### cadmpeg-codec-rhino

#### RHINO4-01 — P2 — Reversed NURBS trim break collection retains overflowing parameter reflection

Locations: `crates/cadmpeg-codec-rhino/src/writer.rs:1336`.

For domain [1e308,1.4e308] and interior knot 1.2e308, break collection inserts infinity although the reflected knot is finite near 1.2e308. Subsequent span sampling can form NaN and refuse the trim. The later edge-point reflection in this same function already calls the corrected shared helper; the break collector was left behind.

Repair: Use reflect_parameter for the break knots too, and keep invalid parameters out of the span list.

Evidence: `more_expressions::rhino_reversed_trim_break`; exact expression and surrounding span-evaluation source review.

### cadmpeg-codec-sldprt

#### SLDPRT4-01 — P2 — Radius keys still alias different large cylinders after coordinate-key repair

Locations: `crates/cadmpeg-codec-sldprt/src/resolved_features/transforms.rs:318`, `crates/cadmpeg-codec-sldprt/src/resolved_features/dimensions.rs:690`.

At quantum 1e-6, radii 1e14 and 2e14 both become i64::MAX. These keys select cylinder-center sets for marker transforms. The prior GridPoint repair preserves coordinate identity, but the radius key in the same workflow still saturates. The key collision is proven; an end-to-end wrong feature binding was not executed.

Repair: Use a checked or identity-preserving scalar radius key consistently on both dimension and cylinder sides.

Evidence: `expressions::radius_keys`; exact key expression plus full selector/source review.

#### SLDPRT4-02 — P2 — Helix circle fitting rejects an exact small helix because its matrix has units

Locations: `crates/cadmpeg-codec-sldprt/src/resolved_features/helix.rs:99`, `crates/cadmpeg-codec-sldprt/src/resolved_features/helix.rs:148`.

Seventeen equally spaced samples of a one-turn helix with radius and pitch 1 pass. Scaling both to 1e-8 makes fit_helix_polyline return None. Circle fitting assembles an unscaled normal matrix and applies an absolute 1e-14 pivot cutoff; its rank decision changes with coordinate units.

Repair: Center and scale the fitting coordinates before forming the circle equations; use a rank test relative to the normalized matrix.

Evidence: `sld::small_helix`; complete extracted helix fitter and its solver helpers.

#### SLDPRT4-03 — P3 — Two remaining 3D distance wrappers repeat Point3::distance

Locations: `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:4939`, `crates/cadmpeg-codec-sldprt/src/resolved_features/sketch_write.rs:829`.

Both functions have byte-equivalent token bodies implementing the same chained hypot expression already owned by Point3. They add no tolerance, conversion or validation contract.

Repair: Call Point3::distance directly and remove the wrappers.

Evidence: exact AST token match plus complete function review.

### cadmpeg-ir

#### IR4-10 — P1 — Parabola coordinate scaling changes the curve parameterization

Locations: `crates/cadmpeg-ir/src/geometry/pcurve.rs:1563`, `crates/cadmpeg-ir/src/geometry/pcurve.rs:1615`.

The method promises to scale chart coordinates without changing parameterization. At t=1 a unit-axis parabola with focal distance 2 evaluates to (0.125,1). Scaling coordinates by [3,3] should give (0.375,3); the method returns a curve that evaluates to (1/24,1). It changes only the vertex and focal distance, leaving the axes and parameter unchanged. STEP pcurve import and Creo unit normalization call this method.

Repair: Scale the parabola polynomial coefficients/axes while keeping t fixed, or represent the coordinate map as a checked transform. Preserve existing parameter ranges.

Evidence: `coordinate_scaling::parabola_parameter_is_not_preserved`; cached public IR scaling method and evaluator, with current method and codec callers source-reviewed.

#### IR4-01 — P2 — Spatial point-distance validation accepts a wrong finite dimension after norm overflow

Locations: `crates/cadmpeg-ir/src/validate/sketches.rs:852`.

Points (0,0,0) and (1e200,0,0) have finite separation 1e200. The squared norm becomes infinity. With expected length 1, the final comparison is infinity <= infinity and accepts the constraint. This is a separate validation branch from the residual paths repaired in IR3-01.

Repair: Use Point3::distance and reject nonfinite measured distances before the relative comparison.

Evidence: `expressions::ir_distance_admission`; exact expressions from the PointDistance branch; full validation traversal not executed.

#### IR4-02 — P2 — Evaluator affine helpers bypass the corrected Transform arithmetic

Locations: `crates/cadmpeg-ir/src/eval.rs:7274`, `crates/cadmpeg-ir/src/eval.rs:7283`.

A transform row [1e308,-1e308,1,0] applied to (2,2,3) should yield x=3. The local evaluator returns NaN, while Transform::apply_point returns 3. These copies serve transformed curves, surface partials and replicas. IR-04 repaired the Transform owner, but these evaluator copies remain.

Repair: Delete the raw affine helpers and route callers through the checked owning Transform operations, propagating an unrepresentable result.

Evidence: `ir_more::affine_cancel`; extracted function.

#### IR4-03 — P2 — Replica orientation treats overflowing and underflowing reflections as positive

Locations: `crates/cadmpeg-ir/src/eval.rs:7262`.

For diag(-a,a,a), a=1e-200 or 1e200, the determinant sign is negative. The helper returns +1 because its raw determinant becomes signed zero or negative infinity. Replica mapping and the fallback oriented normal consume this sign.

Repair: Determine determinant sign with scaled or exact products; do not substitute positive orientation when magnitude is outside f64 range.

Evidence: `ir_more::reflection_orientation`; extracted function.

#### IR4-04 — P2 — The 2D rational differential still accumulates unscaled homogeneous products

Locations: `crates/cadmpeg-ir/src/eval.rs:1977`.

A degree-one pcurve with poles (1e200,0),(2e200,0), weights (1e200,1e200), and knots [0,0,1,1] has midpoint (1.5e200,0) and derivative (1e200,0). pcurve_uv returns None after weighted products overflow. This is the 2D evaluator, distinct from the 3D point/jet paths repaired under IR-02.

Repair: Share the robust homogeneous accumulation and quotient-derivative policy with the 3D owner, preserving the 2D output type.

Evidence: `iges_eval::weighted_nurbs_overflow`; current-source review plus cached public IR pcurve evaluator; library hash recorded.

#### IR4-05 — P2 — Polar pcurve derivatives depend on arbitrary radial scale

Locations: `crates/cadmpeg-ir/src/eval.rs:7507`, `crates/cadmpeg-ir/src/eval.rs:7568`.

For radial coefficients a*(cos t,sin t), the angular curve is u=t at every positive a. At a=1e-200 the harmonic path returns no point; at a=1e200 it returns no tangent. The expected tangent is (1,0). The PolarNurbs branch repeats the unscaled angular quotient and squared denominator.

Repair: Use one scaled angular value/first/second derivative implementation for both radial representations.

Evidence: `ir_more::polar_scale`; cached public IR evaluator reproduces PolarHarmonic; PolarNurbs copy source-reviewed.

#### IR4-06 — P2 — Great-circle latitude differentiation drops a representable derivative

Locations: `crates/cadmpeg-ir/src/eval.rs:7622`.

At plane_slope=1e200, azimuth_rate=1 and t=0.5, the latitude derivative is approximately -6.225e-201. Squaring plane_slope makes the denominator infinite and returns negative zero. The acceleration has additional overflowing products.

Repair: Scale the rational derivative before multiplication and preserve the finite chain derivatives.

Evidence: `ir_more::great_circle_derivative`; cached public IR pcurve evaluator; source arithmetic reviewed.

#### IR4-07 — P2 — Remaining unary laws reject or erase finite chain derivatives

Locations: `crates/cadmpeg-ir/src/eval.rs:4924`.

LN at x=dx=1e-310 should have derivative 1 but returns None. COT and CSC at x=dx=1e-200 should have derivative about -1e200 but return None. ARCSECH at x=dx=1e-310 has finite value and derivative about -1 but returns None. EXP(-750), chained with dx=1e300, returns derivative 0 instead of about 1.90e-26. These are operators not repaired by IR3-02.

Repair: Combine the operand derivative with the reciprocal or exponential before range loss; use a logarithmic form for ARCSECH near zero.

Evidence: `ir::remaining_chain_rules`; extracted function.

#### IR4-08 — P2 — Polyline interpolation returns a nonfinite point for a finite midpoint

Locations: `crates/cadmpeg-ir/src/eval.rs:7199`, `crates/cadmpeg-ir/src/eval.rs:7220`.

The midpoint of endpoints (-1e308,0,0),(1e308,0,0) over [0,1] is the origin; polyline_point returns Some(infinity). A unit segment over parameter bounds [-1e308,1e308] returns None at parameter 0 because the span overflows. The tangent has the same unscaled differences.

Repair: Use overflow-safe normalized parameter ratios and interpolation; apply the same scaling to the tangent quotient and enforce finite outputs.

Evidence: `ir_more::polyline_interpolation`; extracted function.

#### IR4-09 — P2 — Shared planar intersections still lose finite answers before their scaled solve

Locations: `crates/cadmpeg-ir/src/math/planar.rs:42`, `crates/cadmpeg-ir/src/math/planar.rs:98`.

A line from (0,0) to (1e-200,0) through a unit circle has finite parameters +/-1e200, but common scaling makes the direction square zero. Two circles centered at +/-1e308 with radius 1e308 touch at the origin; raw center subtraction overflows and the helper returns None. This is a remaining dynamic-range limit in the newly shared implementation, not the earlier discriminant false-tangent bug.

Repair: Scale positions before subtracting overflowing differences and scale line direction independently from circle geometry.

Evidence: `planar::finite_intersections_lost`; extracted function.

## Other observations and exclusions

- Remaining token duplicates include point/vector component comparisons, mutable/immutable accessors, wire conversion arms, enum counters and endian-specific readers. Their signatures, types or wire policies differ. A new generic trait solely to remove these small bodies would not remove a meaningful policy copy.
- Raw squared distances also remain in search heuristics and bounded source-unit checks. A suspicious square alone is not evidence of a wrong admitted model. Only a demonstrated violating decision or a concrete consolidation into an existing owner is ranked.
- The F3D body/face attribute fallback sequences repeat the same order. They remain a low-confidence consolidation candidate because their owner-specific lookup paths would require additional dispatch or callback plumbing. No behavior bug was established.
- Rhino's remaining squared trim residual was reviewed. The minimum write tolerance defeats the proposed subnormal-gap false-acceptance case; it was not ranked. Large residuals could still cause conservative rejection, but no complete valid trim fixture was executed here.
- The shared planar fixes still need the dynamic-range cases in IR4-09. These findings do not invalidate the prior discrimination and witness fixes; they identify different finite inputs those fixes do not cover.
