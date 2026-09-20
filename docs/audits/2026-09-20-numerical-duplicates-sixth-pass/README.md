# Sixth numerical and duplicate-function audit

Branch: `feat/finish-illegal-states`. Closing HEAD: `dbf8b3f47e4de79f34e4e66394ac29f6bde4ed08`. The checkout had concurrent changes; findings refer to the source fingerprints in the evidence manifest. This audit changed no production source. This directory preserves its report and evidence.

**19 new ranked observations: 16 numerical defects and 3 duplication observations.** All 21 crate directories were screened (20 workspace members plus the excluded fuzz crate). No crate reached the requested maximum of 30 items. Related copies are grouped when they share one repair. These are additional findings after the preceding fixes, not a claim that those earlier witnesses still fail.

Priority 1: wrong replacement of a geometric carrier. Priority 2: wrong numerical result, admission, or loss of representable geometry. Priority 3: maintenance duplication with no demonstrated runtime defect. Extreme-exponent triggers are stated explicitly; they have lower practical priority than wrong geometry at ordinary scales.

## Per-crate results

| Crate | Functions in screening census | Numerical | Duplication | Total |
|---|---:|---:|---:|---:|
| `cadmpeg` | 390 | 0 | 0 | 0 |
| `cadmpeg-asm` | 592 | 1 | 0 | 1 |
| `cadmpeg-codec-catia` | 2,024 | 1 | 0 | 1 |
| `cadmpeg-codec-creo` | 2,341 | 4 | 0 | 4 |
| `cadmpeg-codec-f3d` | 3,211 | 1 | 1 | 2 |
| `cadmpeg-codec-freecad` | 877 | 0 | 1 | 1 |
| `cadmpeg-codec-iges` | 1,102 | 1 | 0 | 1 |
| `cadmpeg-codec-inventor` | 539 | 0 | 0 | 0 |
| `cadmpeg-codec-nx` | 3,038 | 1 | 0 | 1 |
| `cadmpeg-codec-rhino` | 1,057 | 2 | 1 | 3 |
| `cadmpeg-codec-sat` | 31 | 0 | 0 | 0 |
| `cadmpeg-codec-sldprt` | 2,320 | 1 | 0 | 1 |
| `cadmpeg-codec-step` | 791 | 0 | 0 | 0 |
| `cadmpeg-container` | 90 | 0 | 0 | 0 |
| `cadmpeg-core` | 284 | 0 | 0 | 0 |
| `cadmpeg-fuzz` | 203 | 0 | 0 | 0 |
| `cadmpeg-ir` | 2,764 | 4 | 0 | 4 |
| `cadmpeg-parasolid` | 23 | 0 | 0 | 0 |
| `cadmpeg-protein` | 37 | 0 | 0 | 0 |
| `cadmpeg-registry` | 67 | 0 | 0 | 0 |
| `cadmpeg-test-support` | 69 | 0 | 0 | 0 |

A zero means no additional ranked finding established in this pass. It does not certify the crate. SAT and other codecs may inherit ASM/IR defects; shared implementations are counted once at their owner.

## Verification and limits

- The starting census parsed 1,225 Rust source files and 21,850 function items without parse errors. Test trees and recognized test functions/modules were excluded. It is a screening census, not a manual proof of every function.
- Candidate selection used exact and identifier-normalized body groups, numeric-pattern screens, current caller inspection, and comparison against the preceding reports. Simple typed accessors, representation conversions, and unit-vector admission gates were not called defects solely for containing repeated syntax or squares.
- [Twenty-one standalone Rust probes](evidence/probes.log) reproduced the stated numerical witnesses. Compiler and probe exits are 0. Probe assertions confirm the bugs; passing is not evidence that they are fixed.
- [Reproducer](evidence/reproduce.py), [first extension](evidence/extra-more.inc), [second extension](evidence/extra-final.inc), [captured Rust](evidence/probes.rs.txt), [source and cached-library fingerprints](evidence/probe-sources.json), [compile command](evidence/probe-build.command), and [machine-readable findings](findings.json) are saved here. The builder extracts current functions; cached cadmpeg-ir supplies data types, constructors and basic math. Minimal container scaffolding and expression-only probes are identified on each finding.
- No Cargo build, workspace test, corpus decode, complete export, or golden regeneration was run. Full codec regressions remain to be added during implementation.
- An initial secant probe expected collapsed roots; Rust produced two inaccurate roots instead. Its final assertion compares against independently derived exact roots, and the initial output remains in probes-initial-20.log.
- Concurrent changes were not staged, committed, reverted, or overwritten. Candidate source hashes were rechecked at close.

## Ranked findings

### cadmpeg-asm

#### ASM6-01 — P1 — Small common weights make a noncircular curve pass exact circle recognition

Numerical. Confidence: High.

Sources: [`rational_four_arc_circle`](../../../crates/cadmpeg-asm/src/brep/geometry.rs#L808), [`reduce_homogeneous_bezier_to_quadratic`](../../../crates/cadmpeg-asm/src/brep/geometry.rs#L956), [`analytic_procedural_surface`](../../../crates/cadmpeg-asm/src/brep/geometry.rs#L586).

**Trigger.** A degree-two closed rounded square with the usual quarter-circle control polygon and all nine weights equal is polynomial, not circular. Its first-span midpoint is (0.75,0.75), radius 1.06066017. Weights all 1 are rejected; weights all 1e-12 return a unit circle. The common weight factor cannot change this curve.

**Effect.** The weight comparison floors its scale at 1, so the wrong middle-to-end weight ratio passes. The extrusion branch uses this result to replace the procedural carrier with a cylinder; the rolling-ball branch uses it for torus recognition. The degree-reduction residual uses the same homogeneous absolute floor and must be addressed with this owner. Helper failure and consumer path are established; no complete ASM input was converted.

**Repair.** Normalize homogeneous weights before recognition and compare scale-free weight ratios. Make degree-reduction residuals invariant to a common homogeneous factor. Keep a noncircular equal-weight counterexample.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `asm::noncircle_small_weights`.

### cadmpeg-codec-catia

#### CATIA6-01 — P2 — Analytic endpoint angles still square the radius before dividing

Numerical. Confidence: High.

Sources: [`standard_analytic_curve_angle`](../../../crates/cadmpeg-codec-catia/src/families/standard/decode.rs#L8055).

**Trigger.** For an admitted origin-centered circle of radius 1e200 or 1e-200, its exact first endpoint (radius,0,0) returns None instead of angle 0. The dot product and squared length overflow or underflow together.

**Effect.** The circle and ellipse parameter-range reconstruction can discard finite endpoints. This requires extreme exponents; the former support-line residual fix does not cover this function.

**Repair.** Project onto unit principal directions, then divide by the respective radii with range-safe arithmetic.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `catia::finite_circle_endpoint_angle`.

### cadmpeg-codec-creo

#### CREO6-01 — P2 — Cylinder–sphere candidates turn disjoint or secant surfaces into a tangent circle

Numerical. Confidence: High.

Sources: [`coaxial_cylinder_sphere_circle_candidates`](../../../crates/cadmpeg-codec-creo/src/decode/surfaces/intersection_candidates.rs#L144).

**Trigger.** A cylinder of radius 2e-6 around a concentric sphere of radius 1e-6 returns one radius-2e-6 circle although they are disjoint. Reversing the radii returns one central circle instead of two circles at z=±sqrt(3)e-6.

**Effect.** The squared-height tolerance is 1e-9 because scale is floored at 1. Candidate existence and multiplicity change. Consumers combine these with the single-component intersection path, then filter candidates against endpoint evidence; those filters cannot recover omitted components. A complete Creo decode was not run.

**Repair.** Compute squared-height signs and tangency in a radius-scaled chart; retain distinct disjoint, tangent and secant outcomes.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `creo::disjoint_cylinder_sphere_invents_circle`, `creo::secant_cylinder_sphere_becomes_tangent`.

#### CREO6-02 — P2 — Cone intersection quadratics still replace two roots with a false tangent

Numerical. Confidence: High.

Sources: [`coaxial_cone_sphere_circle_candidates`](../../../crates/cadmpeg-codec-creo/src/decode/surfaces/intersection_candidates.rs#L526), [`coaxial_cone_torus_circle_candidates`](../../../crates/cadmpeg-codec-creo/src/decode/surfaces/intersection_candidates.rs#L605).

**Trigger.** For a 45-degree coaxial cone with reference radius 1e-6 and sphere radius 2e-6, the helper returns one circle at z=-5e-7, radius 5e-7; that circle is not on the sphere. For cone reference radius 3e-6 and a torus with major/minor radii 3e-6/1e-6, it returns the central radius-3e-6 circle, not the two real crossings.

**Effect.** The discriminant scale has a 1 floor, so dimensional discriminants near 1e-11 are classified as zero. These copies do not use the previously repaired quadratic owner.

**Repair.** Normalize the meridian chart and route the quadratic solve through the checked quadratic arithmetic. Forward-check returned circles on both supports.

**Evidence scope.** Extracted functions; a minimal circular-cone scaffold supplies the same constant origin, axes, radius and 45-degree angle as the production equation type.

Probe(s): `creo::cone_false_tangent_circles`.

#### CREO6-03 — P2 — Two torus candidate paths invent circles for disjoint supports

Numerical. Confidence: High.

Sources: [`coaxial_cylinder_torus_circle_candidates`](../../../crates/cadmpeg-codec-creo/src/decode/surfaces/intersection_candidates.rs#L705), [`axis_normal_plane_torus_circle_candidates`](../../../crates/cadmpeg-codec-creo/src/decode/surfaces/intersection_candidates.rs#L775).

**Trigger.** For a torus with major radius 3e-6 and minor radius 1e-6, both a coaxial cylinder of radius 5e-6 and a horizontal plane at z=2e-6 are disjoint. Each helper returns one circle.

**Effect.** Both floor the squared-height tolerance at 1e-9 and turn a negative height into tangency. These are independent copies outside the repaired meridian circle–circle helper.

**Repair.** Use dimensionless squared-height comparisons after scaling by the participating radii and displacement.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `creo::torus_fabricated_intersections`.

#### CREO6-04 — P2 — Small secant cylinders and plane–cylinder pairs lose both generator lines

Numerical. Confidence: High.

Sources: [`parallel_cylinder_generator_candidates`](../../../crates/cadmpeg-codec-creo/src/decode/surfaces/intersection_candidates.rs#L81), [`parallel_plane_cylinder_generator_candidates`](../../../crates/cadmpeg-codec-creo/src/decode/surfaces/intersection_candidates.rs#L27).

**Trigger.** Two parallel cylinders of radius 1e-6 separated by 1e-6 return zero generators instead of two. A central plane cutting a radius-1e-10 cylinder also returns zero instead of two.

**Effect.** The remaining dimensional height gates use an absolute floor after scaling by max(1). The cylinder case is the more consequential trigger; the plane case requires a much smaller radius.

**Repair.** Use normalized cross-section geometry and distinguish a zero-height tangent from a finite secant. Preserve the two generator candidates.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `creo::small_cylinder_secants_lost`, `creo::plane_cylinder_secants_lost`.

### cadmpeg-codec-f3d

#### F3D6-01 — P2 — Sketch reflection produces nonfinite points for finite exact reflections

Numerical. Confidence: High.

Sources: [`reflect_point`](../../../crates/cadmpeg-codec-f3d/src/design/dimensions.rs#L6088).

**Trigger.** Reflecting (1e200,1) in the X axis represented by (0,0)–(1e200,0) yields (NaN,NaN). Reflecting the fixed point (1e308,1) in the vertical line x=1e308 yields an infinite x coordinate.

**Effect.** The symmetry relation inference loses valid geometry: the squared direction overflows, and the separate 2*projected-point expression overflows even for an unchanged point.

**Repair.** Normalize the axis and compute reflection as a checked displacement from the input point; avoid squaring the raw axis or doubling the absolute projected position.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `f3d::reflection_axis_length_overflow`, `f3d::reflection_finite_fixed_point`.

#### F3D6-02 — P3 — Body and face attribute selection repeat the same precedence algorithm

Duplication. Confidence: High duplication; medium refactor benefit.

Sources: [`owner_color_or_body_tag_ref`](../../../crates/cadmpeg-codec-f3d/src/writer/generate/attributes.rs#L462), [`owner_color_or_face_tag_ref`](../../../crates/cadmpeg-codec-f3d/src/writer/generate/attributes.rs#L540).

**Trigger.** Both select color first, then name, persistent tags, timestamp, and finally -1. Their bodies are structurally identical after substituting the owner-specific functions and target type.

**Effect.** The source-less writer maintains the same precedence contract twice. This is maintenance duplication, not a demonstrated output defect.

**Repair.** Share the precedence operation using explicit lazy owner-specific lookups, preserving error propagation and short-circuit order. Keep face/body offset calculations owned by their respective functions.

**Evidence scope.** Source and normalized-token comparison.

### cadmpeg-codec-freecad

#### FREECAD6-01 — P3 — 2D and 3D curve censuses repeat recursive wrapper accounting

Duplication. Confidence: High duplication; medium refactor benefit.

Sources: [`census_curve2d`](../../../crates/cadmpeg-codec-freecad/src/brep.rs#L2069), [`census_curve`](../../../crates/cadmpeg-codec-freecad/src/brep.rs#L2091).

**Trigger.** Both classify the same six leaf families, increment trimmed/offset wrapper counts, recurse into the basis, and increment the leaf count. The type and recursive function names are the only structural substitutions.

**Effect.** The census accounting policy has two copies. The 2D and 3D enums are legitimate separate types; no incorrect census was demonstrated.

**Repair.** Consolidate the wrapper-counting walk only if an existing curve view can expose family and basis without a new public abstraction. Do not merge the two source grammars.

**Evidence scope.** Source and normalized-token comparison.

### cadmpeg-codec-iges

#### IGES6-01 — P2 — Parabola focal length still depends on a common coefficient multiplier

Numerical. Confidence: High.

Sources: [`project`](../../../crates/cadmpeg-codec-iges/src/entities/conics.rs#L100).

**Trigger.** The exact focal expressions in both parabola orientations give 0.25 for coefficients 1 and -1, but 0 for the same coefficients multiplied by 1e308. The factor 4*A or 4*C overflows before division. The curve and endpoints can remain ordinary unit-scale geometry.

**Effect.** Type 104 parabola construction then refuses the zero focal distance. The earlier coefficient-sign/classification repair does not fix these two branches.

**Repair.** Use a range-safe quotient/product for focal distance, including placement scale factors; audit the subsequent 2*focal-distance parameter division in the same change.

**Evidence scope.** Both exact focal-distance expressions extracted from project; no complete IGES record/decode fixture.

Probe(s): `iges::parabola_common_coefficient_scale`.

### cadmpeg-codec-nx

#### NX6-01 — P2 — Analytic pcurve reversal still loses finite representations

Numerical. Confidence: High.

Sources: [`reverse_analytic_pcurve_over_range`](../../../crates/cadmpeg-codec-nx/src/decode/pcurves.rs#L884).

**Trigger.** A line with direction (0.5,0), origin zero and range [1e308,1.4e308] returns None although its reversed origin 1.2e308 is finite. Hyperbola and hyperbolic pcurves with coefficients/radii 1e-10 over [359,361] also return None: cosh(720) overflows although the scaled reversed coefficients are about 2.46035e302.

**Effect.** The NURBS and parabola reversal repairs did not cover this remaining analytic helper. Avoidable intermediate overflow drops otherwise representable reversed pcurves.

**Repair.** Compute reflected coefficients through checked products and sums; use scaled sinh/cosh products for both hyperbolic variants. Keep the exact representation when its coefficients are finite.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `nx::reversal_finite_line_origin`, `nx::reversal_finite_hyperbolic_coefficients`.

### cadmpeg-codec-rhino

#### RHINO6-01 — P2 — Solid orientation changes when a tetrahedron is translated

Numerical. Confidence: High.

Sources: [`planar_solid_orientation`](../../../crates/cadmpeg-codec-rhino/src/writer.rs#L1056).

**Trigger.** An outward tetrahedron with vertices (0,0,0),(2,0,0),(0,1,0),(0,0,1) returns solid code 1. Translating every vertex by (1e8,1e8,1e8) returns code 0 with unchanged topology and nonzero volume 1/3.

**Effect.** The writer emits this result into the native solid field. Its absolute-coordinate triple products cancel, so a closed oriented solid is written as unknown/non-solid orientation. The probe uses the exact function and a minimal loop/endpoint model, not a full archive export.

**Repair.** Accumulate signed volume relative to a local reference point, with scale-safe products and stable summation.

**Evidence scope.** Exact function with minimal WritableModel fields and directed tetrahedron loop/endpoint scaffolding.

Probe(s): `rhino::solid_orientation_lost_on_translation`.

#### RHINO6-02 — P2 — Legacy vertex tolerance reconstruction can turn a finite gap into infinity

Numerical. Confidence: High.

Sources: [`parse_legacy_major2`](../../../crates/cadmpeg-codec-rhino/src/brep.rs#L1069).

**Trigger.** The exact endpoint-gap expression turns delta (1e200,0,0) into infinite tolerance instead of finite 1e200. The previous scale-safe vertex average does not change this later squared-distance calculation.

**Effect.** The generated tolerance is stored on the vertex; RawBrep validation requires finite vertex tolerances. A valid representable tolerance can thus be replaced with an invalid one. A full legacy archive fixture was not constructed.

**Repair.** Use the shared scale-safe distance or chained hypot when accumulating endpoint-gap tolerance.

**Evidence scope.** Exact tolerance expression extracted from the legacy parser; storage and finite-tolerance consumer verified in source.

Probe(s): `rhino_tolerance::finite_vertex_tolerance_overflow`.

#### RHINO6-03 — P3 — History and morph property formatting duplicate comma-list serialization

Duplication. Confidence: High duplication; modest refactor benefit.

Sources: [`list`](../../../crates/cadmpeg-codec-rhino/src/history.rs#L669), [`numbers`](../../../crates/cadmpeg-codec-rhino/src/morph.rs#L413).

**Trigger.** The function bodies are token-identical: iterate, ToString, collect and comma-join. The history function is generic; the morph function fixes f64.

**Effect.** This is a small formatting duplication. No numerical or wire discrepancy is demonstrated.

**Repair.** If consolidated, place the formatter in the owning shared wire/property formatting module and import it directly from both callers; do not route morph through the history module.

**Evidence scope.** Exact-token and source comparison.

### cadmpeg-codec-sldprt

#### SLDPRT6-01 — P2 — The remaining line-angle owner erases a resolvable shallow angle

Numerical. Confidence: High.

Sources: [`line_line_angle`](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs#L2249).

**Trigger.** Unit-scale lines with directions (1,0) and (1,1e-8) return angle 0 rather than atan(1e-8), approximately 1e-8 radians. That error exceeds the callers’ 1e-9 angle comparison threshold.

**Effect.** Normalization solved exponent overflow, but acos(dot) loses shallow-angle resolution. Both dynamic relation matching and relation_loci callers use this owner.

**Repair.** Use atan2 of the unit-direction cross magnitude and dot product, preserving the intended unsigned 0..pi range.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `sld::shallow_angle_erased`.

### cadmpeg-ir

#### IR6-01 — P2 — Shared line–circle solver invents tangency and misplaces finite roots after cancellation

Numerical. Confidence: High.

Sources: [`line_circle_parameters`](../../../crates/cadmpeg-ir/src/math/planar.rs#L69).

**Trigger.** For the unit circle at the origin, the line from (1e8,2) to (1e8+1,2) returns two equal parameters -1e8 although the line misses the circle. For the line on y=0 with the same x endpoints, the returned crossings are about ±1.05367 rather than ±1 in model x; the analytical parameters are -100000001 and -99999999.

**Effect.** Scaling prevents overflow but does not recover the small perpendicular residual lost by b²-4ac cancellation. The broad discriminant error allowance admits the missed circle. Shared callers inherit false or inaccurate candidates; no claim that every caller exports them unchecked.

**Repair.** Base the solve on a scaled perpendicular distance and chord half-length, using robust dot/cross arithmetic and a forward residual check.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `ir_planar::remote_line_invents_tangent`, `ir_planar::remote_secant_inaccurate_roots`.

#### IR6-02 — P2 — Planar-line validation accepts perpendicular lines as parallel

Numerical. Confidence: High.

Sources: [`planar_parallel_lines`](../../../crates/cadmpeg-ir/src/validate/sketches.rs#L1500).

**Trigger.** Lines (0,0)–(1e200,0) and (0,1)–(0,1e200) return parallel distance 1. They are perpendicular. Both the cross product and its tolerance overflow to infinity; the rejection comparison is false.

**Effect.** This remaining copy feeds ParallelLineSetDistance validation. The earlier SolidWorks line-distance correction did not update this owner.

**Repair.** Test the cross product of unit directions and compute distance from a unit normal with finite-result admission.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `ir_parallel::perpendicular_admitted_parallel`.

#### IR6-03 — P2 — Parallel-line span overlap turns separated intervals into overlapping intervals

Numerical. Confidence: High.

Sources: [`planar_parallel_line_span_distance`](../../../crates/cadmpeg-ir/src/validate/sketches.rs#L1544).

**Trigger.** Horizontal segments (0,0)–(1e200,0) and (2e200,1)–(3e200,1) return a span distance of 1 although their projected spans are disjoint.

**Effect.** Projection multiplies absolute positions by the unnormalized direction. Endpoints become infinities and the interval test accepts infinity <= infinity. This is a separate remaining defect even after correcting the parallelism helper.

**Repair.** Project relative positions onto a unit direction, preserve ordering at wide spans, and reject nonfinite projection results.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `ir_parallel::separated_spans_admitted_overflow`.

#### IR6-04 — P2 — Fitted-offset frame admission accepts perpendicular endpoint tangents

Numerical. Confidence: High.

Sources: [`fitted_nurbs_offset_candidate`](../../../crates/cadmpeg-ir/src/eval.rs#L1953).

**Trigger.** Source endpoints (0,0),(1,0) with horizontal tangents (1e200,0), and result endpoints (0,1),(1,1) with vertical tangents (0,1e200), produce Some(1). The endpoint tangent contract requires parallel tangents.

**Effect.** The parallel residual becomes infinity/infinity = NaN and the greater-than rejection does not reject it. The public fitted-offset-frame routine and sketch validation consume this candidate; the probe isolates the endpoint-frame check, not interior curve equivalence.

**Repair.** Normalize tangents before comparing their cross product and computing normal/tangential projections. Require finite residuals.

**Evidence scope.** Current function extracted unchanged into a standalone Rust probe.

Probe(s): `ir_offset::perpendicular_offset_frames_admitted`.

## Checked candidates not ranked as new defects

- The Inventor residual floor and SolidWorks origin/matrix quantization changes from the prior pass remain in place.
- CATIA/Fusion saturated spatial-hash cells still have distance checks; no false geometric merge was established from cell saturation alone.
- IGES similarity_orientation already scales its columns before norm/determinant calculations; the raw square-root occurrence is not a new scale bug.
- NX chart_ext_point_at and Fusion line_scalar_count require unit vectors. Rejecting huge non-unit vectors there is correct; a squared norm alone does not establish a defect.
- The IR rational chord-bound underflow candidate has a separate absolute rounding margin. The proposed tiny-residual counterexample does not violate the bound, so it was rejected.
- The remaining mutable/immutable accessors and typed wire conversions often have identical bodies because their types or borrowing contracts differ. Their token equality is not a reason to add traits or wrappers.

## Reproduction

Run `python3 docs/audits/2026-09-20-numerical-duplicates-sixth-pass/evidence/reproduce.py` with `tree_sitter` and `tree_sitter_rust` available to Python, `rustc` on PATH, and a compatible cached `cadmpeg_ir` library under `target/debug/deps`. The script writes to a new temporary directory. It does not invoke Cargo. It extracts current source, so repaired findings can make these bug-confirming assertions fail. The captured Rust and logs preserve the original audit result independently of later edits.

The saved compile command and cached-library path identify the original local evidence. They are provenance, not portable paths. Use the reproducer to derive paths for another checkout.
