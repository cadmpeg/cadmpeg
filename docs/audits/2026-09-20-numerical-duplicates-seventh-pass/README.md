# Seventh numerical and duplicate-function audit

Branch: `feat/finish-illegal-states`. Report source checkpoint: `e3fc23b5edf13edb9f1186b3faa15bd922dd6516`. Concurrent edits were present; individual finding functions and probe source files are fingerprinted. This report makes no production changes.

**16 ranked observations: 14 demonstrated numerical defects, one duplication family, and one tolerance-policy concern.** All 21 crate directories were screened. No crate reached the requested cap of 30. Related copies are grouped by the repair they require.

The first priorities are CATIA's short-arc replacement, FreeCAD's parameter-range collapse, and the separate Fusion/SolidWorks small-angle paths. These change geometry or reject valid data. Extreme exponent and tiny physical-length triggers are stated below and have lower practical priority. P1 means wrong geometric replacement; P2 means incorrect result/admission or a shared representation-dependent solver; P3 means extreme-only range failure, maintenance duplication, or an unresolved approximation policy.

## Per-crate results

| Crate | Functions screened | Numerical | Duplication | Policy | Total |
|---|---:|---:|---:|---:|---:|
| `cadmpeg` | 390 | 0 | 0 | 0 | 0 |
| `cadmpeg-asm` | 592 | 0 | 0 | 1 | 1 |
| `cadmpeg-codec-catia` | 2,024 | 1 | 0 | 0 | 1 |
| `cadmpeg-codec-creo` | 2,341 | 3 | 1 | 0 | 4 |
| `cadmpeg-codec-f3d` | 3,210 | 1 | 0 | 0 | 1 |
| `cadmpeg-codec-freecad` | 871 | 2 | 0 | 0 | 2 |
| `cadmpeg-codec-iges` | 1,102 | 0 | 0 | 0 | 0 |
| `cadmpeg-codec-inventor` | 539 | 0 | 0 | 0 | 0 |
| `cadmpeg-codec-nx` | 3,038 | 1 | 0 | 0 | 1 |
| `cadmpeg-codec-rhino` | 1,056 | 1 | 0 | 0 | 1 |
| `cadmpeg-codec-sat` | 31 | 0 | 0 | 0 | 0 |
| `cadmpeg-codec-sldprt` | 2,320 | 1 | 0 | 0 | 1 |
| `cadmpeg-codec-step` | 791 | 1 | 0 | 0 | 1 |
| `cadmpeg-container` | 90 | 0 | 0 | 0 | 0 |
| `cadmpeg-core` | 284 | 0 | 0 | 0 | 0 |
| `cadmpeg-fuzz` | 203 | 0 | 0 | 0 | 0 |
| `cadmpeg-ir` | 2,780 | 3 | 0 | 0 | 3 |
| `cadmpeg-parasolid` | 23 | 0 | 0 | 0 | 0 |
| `cadmpeg-protein` | 37 | 0 | 0 | 0 | 0 |
| `cadmpeg-registry` | 60 | 0 | 0 | 0 | 0 |
| `cadmpeg-test-support` | 69 | 0 | 0 | 0 | 0 |

Zero means no additional ranked finding established in this pass. It does not certify a crate. Shared defects are counted at their production owner, not again in every dependent codec. [Coverage details](coverage.json) list the focus for each crate.

## Evidence and limits

- The closing AST census parsed 1,225 source files and 21,851 function items without parse errors. Recognized test trees and test-only functions/modules were excluded. This is a screening census, not a manual proof of every function. Macro-generated functions are outside its body comparison.
- Selection used exact and identifier-normalized body comparison, numerical-pattern screening, caller and admission inspection, and comparison with prior reports. Small typed getters, mutable/immutable accessor pairs and representation-specific conversions are not defects merely because their syntax repeats.
- [Fifteen standalone Rust probes](evidence/probes.log) reproduce the numerical behaviors, including the one policy-dependent case. Compile and probe exit statuses are 0. The tests assert the current defects; passing does not mean the defects are fixed.
- [Reproducer](evidence/reproduce.py) and [additional probes](evidence/extra.inc) extract the current production bodies. A cached IR library supplies checked constructors and primitive geometry types. [Captured Rust](evidence/probes.rs.txt), [source/library hashes](evidence/probe-sources.json), commands and exit files preserve the evidence. Each finding states any scaffold or expression-only extraction. This verifies the cited arithmetic, not a complete codec transaction.
- Run the reproducer from the repository root with `tree-sitter` and `tree-sitter-rust` installed and a cached `target/debug/deps/libcadmpeg_ir*.rlib`. It writes a fresh temporary evidence directory and does not invoke Cargo. The saved cached-library hash identifies the library used here; a different local cache is a new run.
- No Cargo build, full crate check, corpus decode/export, workspace tests or golden regeneration was run for this report. Full owner regressions belong in a subsequent implementation change.
- [Machine-readable findings](findings.json) include the function hashes, independent triggers, effects, repair boundaries and relation to previous repairs. [Census source hashes](evidence/sources.json) and [body groups](evidence/shape-groups.txt) preserve screening inputs. Current candidate function hashes were checked before commit. Concurrent agents' source changes were left untouched.

## Ranked findings

### cadmpeg-asm

#### ASM7-01 — P3 — A relatively curved tiny spine is classified as linear

Policy. Confidence: Behavior confirmed; defect classification depends on the intended approximation contract. Rank 1 in this crate.

Sources: [`linear_nurbs_spine`](../../../crates/cadmpeg-asm/src/brep/geometry.rs#L761), [`analytic_rolling_ball_surface`](../../../crates/cadmpeg-asm/src/brep/geometry.rs#L621).

**Trigger.** A quadratic Bezier spine with poles (0,0,0),(5e-11,4e-11,0),(1e-10,0,0) is accepted as an X-axis line. Its midpoint is (5e-11,2e-11,0): 20% of the chord off that line.

**Effect.** The collinearity tolerance is 1e-10*max(extent,1). It dominates this entire spine and can enable cylinder classification. The absolute error is only 2e-11 mm, so whether this is wrong depends on whether this owner promises exact analytic recognition or an allowed absolute approximation.

**Repair boundary.** Decide and document that contract. For exact recognition, use a scale-relative criterion or source error bounds. If absolute approximation is intended, retain it explicitly and do not describe the replacement as exact.

**Evidence.** asm::curved_spine_becomes_line; complete extracted classifier and checked NURBS evaluation.

**Limits.** Policy-dependent observation, not counted as a confirmed numerical defect. Tiny physical dimensions; downstream complete rolling-ball transfer was not executed.

**Previous work.** Different owner or failure mechanism from the earlier reported witnesses.

### cadmpeg-codec-catia

#### CATIA7-01 — P1 — A short witnessed arc is replaced by a full circle

Numerical. Confidence: High. Rank 1 in this crate.

Sources: [`standard_analytic_curve_parameter_range`](../../../crates/cadmpeg-codec-catia/src/families/standard/decode.rs#L8081).

**Trigger.** A unit circle with endpoints at angles 0 and 0.001 radians and an interior witness at 0.0005 returns [0, TAU]. Its endpoint separation is about 0.001 mm, below the 0.002 mm endpoint threshold.

**Effect.** The proximity shortcut runs before angular or witness checks. A small edge can gain almost one full turn; the witness already distinguishes the short arc. This is not a last-bit normalization difference.

**Repair boundary.** Use source closure information or a distinct angular/witness decision for full turns. Endpoint proximity alone cannot establish closure.

**Evidence.** catia::short_witnessed_arc_becomes_full_circle; complete extracted function and its actual helpers, checked IR circle.

**Limits.** No complete codec decode or export was run.

**Previous work.** The sixth-pass repair stabilized standard_analytic_curve_angle at extreme radii. This is the caller’s separate full-turn shortcut.

### cadmpeg-codec-creo

#### CREO7-01 — P2 — Plane–circle intersection returns the circle center for a secant

Numerical. Confidence: High. Rank 1 in this crate.

Sources: [`intersect_plane_with_circle`](../../../crates/cadmpeg-codec-creo/src/decode/analytic/equations.rs#L1187).

**Trigger.** For circle center (0,0,0), axis Z, radius 1e-7, and plane x=0, the function returns only (0,0,0). The exact intersections are (0,+1e-7,0) and (0,-1e-7,0).

**Effect.** The squared radial remainder is compared with 1e-12*max(radius,1)^2 and classified as tangent. The result is not on the circle. intersection_resolve uses this owner when cutting carrier-intersection circles by a plane.

**Repair boundary.** Use a dimensionless radial comparison and a stable half-chord; classify multiplicity independently of a 1 mm floor.

**Evidence.** creo::tiny_plane_circle_returns_center; complete extracted function with production vector helpers.

**Limits.** No complete codec decode or export was run.

**Previous work.** Earlier fixes covered trim/sketch circle solvers and surface candidate constructors. This is the separate analytic plane-cut owner.

#### CREO7-02 — P2 — Tiny disjoint sphere supports manufacture a tangent point

Numerical. Confidence: High. Rank 2 in this crate.

Sources: [`tangent_sphere_point`](../../../crates/cadmpeg-codec-creo/src/decode/analytic/planes.rs#L95), [`tangent_plane_sphere_point`](../../../crates/cadmpeg-codec-creo/src/decode/analytic/planes.rs#L116), [`point_on_carrier`](../../../crates/cadmpeg-codec-creo/src/decode/analytic/planes.rs#L42).

**Trigger.** Radius-1e-10 spheres at x=0 and x=3e-10 return the midpoint x=1.5e-10 although they are disjoint. A sphere at x=3e-10 with radius 1e-10 and plane x=0 returns the origin. point_on_carrier accepts these false candidates against the sphere supports.

**Effect.** The pair constructors use an absolute 1e-9 floor and the final carrier gate uses an even larger absolute 1e-7 floor. solve_carriers_with_diagnostics therefore cannot remove these false pair candidates at its final membership check.

**Repair boundary.** Use support-scaled tangency and membership residuals. Any absolute model tolerance must be explicit and must not silently turn a disjoint pair into a unique exact tangent.

**Evidence.** creo::disjoint_tiny_spheres_invent_tangent; complete extracted tangent and membership functions.

**Limits.** Very small model lengths; constructor and final membership behavior reproduced. The complete triple-carrier solver was inspected, not executed.

**Previous work.** Different owner or failure mechanism from the earlier reported witnesses.

#### CREO7-03 — P3 — Carrier membership retains an overflowing squared norm

Numerical. Confidence: High. Rank 3 in this crate.

Sources: [`point_on_carrier`](../../../crates/cadmpeg-codec-creo/src/decode/analytic/planes.rs#L42).

**Trigger.** The exact point (1e200,0,0) on a radius-1e200 sphere centered at the origin is rejected because dot(relative,relative).sqrt() is infinite.

**Effect.** Valid candidates are rejected by the common final membership gate. Cylinder radial distance and the torus tube-distance arm contain the same unscaled squared-norm construction.

**Repair boundary.** Use robust norms in each arm and stable radial projection where needed. Keep this range repair separate from the small-radius tolerance policy.

**Evidence.** creo::large_sphere_point_is_refused; complete function, sphere arm executed; cylinder and torus copies confirmed in source.

**Limits.** Extreme finite model coordinates. Only the sphere witness was executed.

**Previous work.** Different owner or failure mechanism from the earlier reported witnesses.

#### CREO7-04 — P3 — Remaining section collectors duplicate an existing collector policy

Duplication. Confidence: High. Rank 4 in this crate.

Sources: [`prototype_pcurves`](../../../crates/cadmpeg-codec-creo/src/container.rs#L1581), [`curve_prototype_topology`](../../../crates/cadmpeg-codec-creo/src/container.rs#L1598), [`curve_topology_rows`](../../../crates/cadmpeg-codec-creo/src/container.rs#L1615), [`curve_prototypes`](../../../crates/cadmpeg-codec-creo/src/container.rs#L1480), [`curve_parameters`](../../../crates/cadmpeg-codec-creo/src/container.rs#L1534), [`curve_expressions`](../../../crates/cadmpeg-codec-creo/src/container.rs#L1497), [`surface_contours`](../../../crates/cadmpeg-codec-creo/src/container.rs#L1353), [`cross_section_surface_contours`](../../../crates/cadmpeg-codec-creo/src/container.rs#L1372), [`cross_section_curve_prototypes`](../../../crates/cadmpeg-codec-creo/src/container.rs#L1644), [`datum_cylinders`](../../../crates/cadmpeg-codec-creo/src/container.rs#L1683), [`collect_section_records`](../../../crates/cadmpeg-codec-creo/src/container.rs#L2770).

**Trigger.** prototype_pcurves and curve_prototype_topology have the same identifier-normalized loop: decode each section, relocate offsets, append, sort. The listed siblings repeat that traversal with different relocation closures or section filters.

**Effect.** collect_section_records already owns this policy and accepts the decoder, relocation and ordering operations. The remaining loops duplicate that owner without adding a traversal guarantee. No runtime defect is asserted.

**Repair boundary.** Route these collectors through the existing helper. Preserve each relocation field and section predicate. two_chart_pcurves can reuse the collection prefix while retaining its distinct uniqueness filter; datum_planes must retain its additional named-plane record.

**Evidence.** Source and AST comparison only; no behavior change or runtime reproduction.

**Limits.** This is one maintenance family, not one finding per function. Typed getters and representation-specific conversions are excluded.

**Previous work.** CREO3-05 and CREO4-02 migrated other collectors. These remaining bodies still contain the common loop.

### cadmpeg-codec-f3d

#### F3D7-01 — P2 — Trace-only angle recovery discards a finite body rotation

Numerical. Confidence: High. Rank 1 in this crate.

Sources: [`matrix_axis_angle`](../../../crates/cadmpeg-codec-f3d/src/design/feature_project.rs#L4076), [`project_move`](../../../crates/cadmpeg-codec-f3d/src/design/feature_project.rs#L3363).

**Trigger.** A proper Z rotation of 1e-8 radians has nonzero off-diagonal sine entries but a trace that rounds to 3. matrix_axis_angle returns None, despite its stated zero-angle gate being 1e-12.

**Effect.** project_move writes rotation=None into MoveBody. The source contains a resolvable rotation, but the feature projection loses it.

**Repair boundary.** Recover the small angle from both the skew part and trace with atan2, while retaining the half-turn axis branch.

**Evidence.** fusion::representable_rotation_is_discarded; extracted function plus checked proper-rigid Transform.

**Limits.** No complete codec decode or export was run.

**Previous work.** The first audit repaired half-turn axis recovery in this function. Small-angle trace cancellation is a different remaining branch.

### cadmpeg-codec-freecad

#### FREECAD7-01 — P2 — Endpoint snapping collapses an exact pcurve domain

Numerical. Confidence: High. Rank 1 in this crate.

Sources: [`normalize_pcurve_parameter_range`](../../../crates/cadmpeg-codec-freecad/src/topology_transfer.rs#L1608), [`bounded_pcurve_range`](../../../crates/cadmpeg-codec-freecad/src/topology_transfer.rs#L1601).

**Trigger.** A valid degree-one pcurve with knots [0,0,1e-10,1e-10] and range [0,1e-10] returns [0,0]. Both endpoints are within the floored 1e-9 tolerance of the lower knot; the lower comparison wins even for an exact upper endpoint.

**Effect.** The stored pcurve range is corrupted. The edge-use path also drops the range because bounded_pcurve_range requires a strict increase. Small parameter intervals do not imply small model geometry: the probe’s poles span one parameter-space unit.

**Repair boundary.** Preserve exact endpoint matches first; then snap to an unambiguous nearest boundary with a tolerance bounded by the domain separation.

**Evidence.** fc_range::exact_small_domain_collapses; complete extracted normalizer and checked PcurveNurbs construction.

**Limits.** No complete codec decode or export was run.

**Previous work.** Different owner or failure mechanism from the earlier reported witnesses.

#### FREECAD7-02 — P3 — Periodic knot extension overflows a cancellable period

Numerical. Confidence: High. Rank 2 in this crate.

Sources: [`normalize_periodic_knots`](../../../crates/cadmpeg-codec-freecad/src/brep.rs#L4787).

**Trigger.** For degree 1 and periodic knots [-1e308,-9e307,9e307,1e308], the function returns Ok with exterior knots -infinity and +infinity. The correct exterior values are -1.1e308 and +1.1e308, both representable.

**Effect.** Computing last-first first overflows, although each final three-term sum is finite. Text and binary curve/pcurve paths and surface knot normalization share this owner; later finite-knot admission refuses the result.

**Repair boundary.** Compute each exterior knot with a scaled or cancellation-safe sum; reject only an unrepresentable final knot.

**Evidence.** fc_periodic::finite_periodic_extension_overflows; complete extracted function, error-message carrier stub.

**Limits.** Extreme finite knot parameters. The full FreeCAD parser was not executed.

**Previous work.** Different owner or failure mechanism from the earlier reported witnesses.

### cadmpeg-codec-nx

#### NX7-01 — P2 — Common rational weight scaling changes closest-point projection

Numerical. Confidence: High. Rank 1 in this crate.

Sources: [`rational_squared_distance_derivative`](../../../crates/cadmpeg-codec-nx/src/decode/blend.rs#L2273), [`homogeneous_residual_distance`](../../../crates/cadmpeg-codec-nx/src/decode/blend.rs#L2610), [`closest_nurbs_curve_parameter_with_budget`](../../../crates/cadmpeg-codec-nx/src/decode/blend.rs#L3751).

**Trigger.** A line from (0,0,0) to (1,0,0), queried at x=0.25, has residual controls [-0.25*w,0,0,w] and [0.75*w,0,0,w]. Common weights 1 give a nonconstant derivative; 1e200 returns None; 1e-200 returns all zeros. The endpoint residual becomes infinity or zero instead of 0.25.

**Effect.** The NURBS geometry is unchanged by a common weight factor. Raw Bernstein products and residual squares make projection depend on that arbitrary representation factor; the production caller accepts these finite positive weights.

**Repair boundary.** Normalize homogeneous controls before polynomial products and use a robust dehomogenized residual norm. Preserve root ordering and the existing work budget.

**Evidence.** nx::common_weight_scale_changes_projection; actual derivative-product and distance helpers; checked NURBS constructor and IR evaluation control.

**Limits.** Extreme common weight factors with ordinary point coordinates. Full budgeted root isolation was not run.

**Previous work.** Different owner or failure mechanism from the earlier reported witnesses.

### cadmpeg-codec-rhino

#### RHINO7-01 — P2 — Shallow extrusion miters lose their tilt through acos

Numerical. Confidence: High. Rank 1 in this crate.

Sources: [`active_miter`](../../../crates/cadmpeg-codec-rhino/src/extrusion.rs#L865), [`mitered_local`](../../../crates/cadmpeg-codec-rhino/src/extrusion.rs#L595).

**Trigger.** active_miter accepts normal (1e-8,0,1). Its normalized Z rounds to 1; acos(Z) becomes zero. Transforming (1e8,0,0) leaves Z=0, with a 1 mm residual against the intended miter plane instead of Z approximately -1.

**Effect.** The input retains a finite transverse normal component, but profile point and basis construction discard the tilt. The error grows with profile extent.

**Repair boundary.** Recover the angle with atan2(hypot(normal.x,normal.y),normal.z), or use a direct stable miter map.

**Evidence.** rhino::shallow_miter_tilt_is_lost; full admission, normalization, miter and Rodrigues helpers; error-carrier stub.

**Limits.** Angle 1e-8 radians. The 1 mm residual witness uses a 1e8 mm profile extent; a 1 mm profile has a 1e-8 mm error.

**Previous work.** Different owner or failure mechanism from the earlier reported witnesses.

### cadmpeg-codec-sldprt

#### SLDPRT7-01 — P2 — Source-less angular validation still rejects valid short and shallow lines

Numerical. Confidence: High. Rank 1 in this crate.

Sources: [`validate_solved_dimension`](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/write_prepare.rs#L738).

**Trigger.** Two perpendicular segments of length 1e-5 are rejected as degenerate because their length product 1e-10 is compared against the linear 1e-9 threshold. Unit-length lines at angle 1e-8 instead measure as zero through acos and fail the final 1e-9 agreement test.

**Effect.** The source-less writer refuses valid dimensions. This is a separate angle implementation from the already repaired relation-record helper, with both a dimensional mismatch and small-angle cancellation.

**Repair boundary.** Check each line length against the linear gate, then compute the angle from normalized cross and dot with atan2. Share the established angle policy where ownership allows.

**Evidence.** sld::valid_angular_dimensions_are_refused; verbatim numeric block from the Angle arm and vector2_length, with a Result wrapper and error stub.

**Limits.** The numeric branch and final comparison were reproduced; complete source-less export was not run.

**Previous work.** SLDPRT6-01 repaired relation_records::line_line_angle. This independent writer branch still uses the old arithmetic.

### cadmpeg-codec-step

#### STEP7-01 — P3 — Finite mesh mass properties are lost when centroid moments overflow

Numerical. Confidence: High. Rank 1 in this crate.

Sources: [`mesh_properties`](../../../crates/cadmpeg-codec-step/src/reader/validation.rs#L330).

**Trigger.** Scale the tetrahedron (0,0,0),(2,0,0),(0,1,0),(0,0,1) by 1e90. Its area, volume (1e270/3), and centroid (5e89,2.5e89,2.5e89) are finite, but mesh_properties returns None because volume-weighted centroid terms overflow.

**Effect.** The validation-property reader loses its computed comparison values. Moving the origin fixed translation cancellation, but does not bound the fourth-power centroid moment intermediates.

**Repair boundary.** Accumulate in a scaled local coordinate system and rescale area, volume and centroid separately; use stable summation for many triangles.

**Evidence.** step::finite_mass_properties_are_lost; complete function with minimal body/mesh containers exposing the same fields and slices. Unit-scale tetrahedron is the control.

**Limits.** Extreme finite model scale. No full STEP validation-property import was run.

**Previous work.** STEP2-01 repaired translation dependence. This is uniform-scale overflow in the remaining moment accumulation.

### cadmpeg-ir

#### IR7-01 — P2 — Circle intersection loses the smaller circle at unequal scales

Numerical. Confidence: High. Rank 1 in this crate.

Sources: [`circle_intersections`](../../../crates/cadmpeg-ir/src/math/planar.rs#L140).

**Trigger.** A unit circle at the origin and a radius-1e200 circle centered at (1e200,0) return a single point (0,0). Analytically they have two intersections near (5e-201,+1) and (5e-201,-1). The returned center is not on the unit circle.

**Effect.** After scaling by the large circle, the small radius is 1e-200 and its square underflows. The height becomes zero and two finite roots collapse to a false tangent. This shared helper is used by codec and IR intersection owners.

**Repair boundary.** Preserve independent radial scales and evaluate the half-chord without squaring away the small radius. Include strongly unequal circles, not only common scaling.

**Evidence.** planar::unequal_circles_intersect_at_wrong_center; complete current function and current finite-sum implementation.

**Limits.** Extreme radius ratio. No claim that ordinary comparable circles fail.

**Previous work.** The earlier planar repair handled common extreme scaling. This witness changes the ratio between the two radii.

#### IR7-02 — P3 — Normalized-vector derivatives are refused after intermediate overflow

Numerical. Confidence: High. Rank 2 in this crate.

Sources: [`unit_vector_with_derivative`](../../../crates/cadmpeg-ir/src/eval.rs#L5346).

**Trigger.** vector=(1,1,1), derivative=(MAX,MAX,MAX) returns None. The normalized vector is constant along this parallel derivative, so its derivative is zero. vector=(MAX,MAX,0) with zero derivative is also refused although its unit direction is finite.

**Effect.** The raw projection unit.dot(derivative), or the full vector length, overflows before a finite normalized result can be formed. Sweep and rolling-ball differential evaluation consume this helper.

**Repair boundary.** Scale the vector and derivative independently and form the projected derivative with stable products and sums; preserve representable final normalized derivatives.

**Evidence.** eval::parallel_derivative_is_refused; complete function plus actual offset/vector-sum helpers.

**Limits.** Extreme finite derivatives or vector magnitudes; no full procedural surface evaluation was run.

**Previous work.** Different owner or failure mechanism from the earlier reported witnesses.

#### IR7-03 — P3 — Sweep rail transforms bypass stable affine arithmetic in two copies

Numerical. Confidence: High. Rank 3 in this crate.

Sources: [`linear_sweep_rail_point`](../../../crates/cadmpeg-ir/src/eval.rs#L5300), [`linear_sweep_rail_vector`](../../../crates/cadmpeg-ir/src/eval.rs#L5308).

**Trigger.** The proper 180-degree rotation about (1,1,1) has diagonal -1/3 and off-diagonal 2/3. Applied to (MAX,MAX,MAX), both helpers produce an infinite Z component. The checked shared Transform applies the same matrix with finite components.

**Effect.** The first two positive terms overflow before the last negative term cancels them. Point and vector copies repeat the same unsafe matrix arithmetic despite an existing stable transform owner.

**Repair boundary.** Reuse the stable transform or finite-dot operation for both point and derivative transformation. Remove the duplicated raw product-sum policy.

**Evidence.** eval::proper_rotation_produces_nonfinite_rail; both complete helpers plus proper-rigid validation and stable-transform comparison.

**Limits.** Coordinates near f64::MAX. The computed final result is representable; this is not rejection of an intrinsically unrepresentable transform.

**Previous work.** Different owner or failure mechanism from the earlier reported witnesses.

## Candidates not promoted to defects

- CATIA and Fusion saturating spatial-cell casts still have distance checks after bucket lookup. They were not treated as identity aliasing without a failing distance predicate.
- NX chart tangents and several SolidWorks stored-frame readers require near-unit vectors. Overflowing norms for out-of-contract huge directions do not establish a valid-input failure there.
- The STEP periodic edge-range arithmetic deserves care for extreme source parameters, but circle parameters on the traced route come from bounded inverse-angle evaluation. No valid complete-route witness was established in this pass.
- Rhino knot classification and IGES similarity orientation already normalize the relevant coordinates before the inspected squares. Their presence in the numerical screen is not evidence of a remaining bug.
- The largest exact-body groups are mainly typed IR accessors. The Creo section loops differ because an existing generic collector already owns precisely their traversal policy; adopting it needs no new trait layer.
