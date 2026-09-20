# Ranked findings

P1: incorrect geometry representation or false boundary certificate. P2: wrong result, failed valid admission/convergence, or lost numerical range. P3: duplicate ownership or an unnecessary layer. Priority is within the demonstrated scope; a helper probe is not claimed as a full-file conversion test.

## cadmpeg-asm

### ASM2-01 — Ellipse and support-cone reconstruction retain manual unsafe lengths

P2 · Numerical · Confidence: High

Sources: [`ellipse_to_nurbs`](../../../crates/cadmpeg-asm/src/nurbs/proc_surface.rs:1623), [`let radius = (major[0] * major[0]`](../../../crates/cadmpeg-asm/src/nurbs/proc_curve.rs:2880).

**Evidence.** ellipse_to_nurbs accepts a unit major radius but rejects 1e200 and 1e-200 with unit normal and ratio 0.5, although all output coordinates can remain finite. The text and binary support-cone branches repeat the same squared-major-length calculation.

**Effect.** The earlier normalized-direction repair did not fix these independent radius computations. Valid extreme-scale ellipse/cone geometry can be dropped. The ellipse is probed; cone branches are confirmed by source inspection.

**Repair direction.** Use scale-safe lengths for major/minor vectors, preserving the native-to-mm conversion and existing geometry admission.

**Probe:** `asm::radii_scale`.

## cadmpeg-codec-creo

### CREO2-01 — Both quadratic solvers change roots when all coefficients are scaled

P2 · Numerical · Confidence: High

Sources: [`quadratic_real_roots`](../../../crates/cadmpeg-codec-creo/src/decode/analytic/equations.rs:516), [`quadratic_roots`](../../../crates/cadmpeg-codec-creo/src/decode/sketch/equations_coordinate.rs:990).

**Evidence.** x^2-1 returns [-1,1]. Multiplying every coefficient by 1e-10 yields [-0] in both solvers; multiplying by 1e-20 yields no roots. Absolute floors control degree and discriminant classification.

**Effect.** Analytic intersection and sketch equation candidates are lost or fabricated by a representation-only scaling. Later geometric checks may reject false candidates, but cannot recover omitted roots.

**Repair direction.** Normalize coefficient magnitude first, use a scale-relative discriminant bound and stable quadratic roots, and retain each caller's admission checks.

**Probe:** `creo::coefficient_scale`.

### CREO2-02 — Circle–circle intersection fabricates a unique tangent point

P2 · Numerical · Confidence: High

Sources: [`trim_circle_circle_intersection`](../../../crates/cadmpeg-codec-creo/src/feature/definitions.rs:3535).

**Evidence.** Two circles of radius 1e-6 with centers (0,0) and (1e-6,0) have two intersections. The function returns the single point (0.5e-6,0), which lies on neither circle.

**Effect.** The scale.max(1) height tolerance turns a genuinely ambiguous trim intersection into a purported unique coordinate.

**Repair direction.** Normalize the geometry and distinguish two crossings from a tangent with a relative error bound; forward-check any returned point against both circles.

**Probe:** `creo::two_circle_intersections`.

### CREO2-03 — Legacy curve and surface array lookup duplicate the same ownership checks

P3 · Duplicate / forwarding layer · Confidence: High

Sources: [`curve_array_elements`](../../../crates/cadmpeg-codec-creo/src/legacy_geometry.rs:202), [`surface_array_elements`](../../../crates/cadmpeg-codec-creo/src/legacy_geometry.rs:377).

**Evidence.** The signatures and lookup/uniqueness/parent/completeness logic are identical. Only the array name, crv_array versus srf_array, differs.

**Effect.** The exact same source ownership invariant has two implementations.

**Repair direction.** Replace the pair with one array lookup parameterized by the array name; update callers directly.

## cadmpeg-codec-f3d

### F3D2-01 — Second line–arc intersection implementation retains the false-tangent bug

P2 · Numerical · Confidence: High

Sources: [`line_arc_intersects`](../../../crates/cadmpeg-codec-f3d/src/design/geometry.rs:2315), [`line_arc_intersection_points`](../../../crates/cadmpeg-codec-f3d/src/design/geometry.rs:672).

**Evidence.** A line from (-1e-4,2e-4) to (1e-4,2e-4) and circle of radius 1e-4 at the origin are disjoint. The boolean routine returns true while the corrected point-producing routine returns no intersections.

**Effect.** Profile independence and containment decisions can reject valid disjoint loops. The .max(1.0) discriminant error floor remains in the boolean copy.

**Repair direction.** Route the boolean decision through the corrected intersection owner and remove the duplicate quadratic solver.

**Probe:** `f3d::copies_disagree`.

### F3D2-02 — Profile NURBS speed bounds fail under common weight scaling

P2 · Numerical · Confidence: High

Sources: [`nurbs_speed_bound`](../../../crates/cadmpeg-codec-f3d/src/design/geometry.rs:1862).

**Evidence.** The same unit pcurve has bound Some(1) with weights [1,1] and None with [1e-200,1e-200]. The unscaled squared minimum weight produces 0/0.

**Effect.** Certified profile-tube and separation paths refuse an unchanged valid profile. This is another local copy of the rational-bound problem.

**Repair direction.** Share a scale-safe bound implementation with the IR where the mathematical contracts agree.

**Probe:** `f3d_speed::weights`.

### F3D2-03 — Canvas and decal scope collection repeat the same algorithm

P3 · Duplicate / forwarding layer · Confidence: High duplication; medium refactor benefit

Sources: [`decode_canvas_images`](../../../crates/cadmpeg-codec-f3d/src/design/decode/canvas.rs:28), [`decode_decal_images`](../../../crates/cadmpeg-codec-f3d/src/design/decode/decal.rs:33).

**Evidence.** Both traverse design bulkstreams, match native stream identities, filter a scope kind, parse matching scopes, and sort/deduplicate image IDs. Their kind, parser, and output record differ.

**Effect.** Stream matching and ordering rules have two maintenance owners. No current behavior error was demonstrated.

**Repair direction.** Factor the shared stream/scope traversal; keep each image parser and record type distinct. Avoid a new public trait solely for this pair.

## cadmpeg-codec-freecad

### FREECAD2-01 — Similarity admission accepts a 60-degree sheared frame

P2 · Numerical · Confidence: High

Sources: [`uniform_scale`](../../../crates/cadmpeg-codec-freecad/src/topology_transfer.rs:1821).

**Evidence.** Columns (1e-6,0,0), (0.5e-6,sqrt(0.75)*1e-6,0), and (0,0,1e-6) pass as a similarity. The first two columns are 60 degrees apart. The dot-product threshold has units of length rather than squared length and floors scale at 1.

**Effect.** ensure_similarity and deflection scaling can accept a transform that changes angles and cannot be represented by one uniform scale.

**Repair direction.** Test orthogonality on normalized columns and compare column lengths with a dimensionally consistent tolerance.

**Probe:** `freecad::small_shear`.

### FREECAD2-02 — Normal transformation silently returns an unnormalized vector

P2 · Numerical · Confidence: High

Sources: [`transform_normalized_vector`](../../../crates/cadmpeg-codec-freecad/src/topology_transfer.rs:1919).

**Evidence.** Uniform transform scales 1e200 and 1e-200 return normal lengths 1e200 and 1e-200 instead of 1. The manual norm overflows/underflows, then the fallback returns the transformed vector unchanged.

**Effect.** Placed triangulation normals violate normalization expectations. Extreme finite scale is required for this witness.

**Repair direction.** Normalize using the shared scale-safe vector operation; do not treat failed normalization as a successful unit vector.

**Probe:** `freecad::transformed_normal`.

### FREECAD2-03 — Three TechDraw list validators duplicate structure admission

P3 · Duplicate / forwarding layer · Confidence: High

Sources: [`validate_gui_geom_format_list`](../../../crates/cadmpeg-codec-freecad/src/gui.rs:2053), [`validate_gui_cosmetic_edge_list`](../../../crates/cadmpeg-codec-freecad/src/gui.rs:2229), [`validate_gui_center_line_list`](../../../crates/cadmpeg-codec-freecad/src/gui.rs:2278).

**Evidence.** The three functions repeat single-root admission, list-tag checks, count parsing, element counting, child tag/type checks, and record dispatch. The literal names and record validator differ.

**Effect.** A structural validation fix requires three parallel edits. Record-specific validation must remain separate.

**Repair direction.** Use one private typed-list traversal with explicit list/record names and a record-validation callback.

### FREECAD2-04 — Edge and vertex appearance transfer repeat style and binding construction

P3 · Duplicate / forwarding layer · Confidence: High duplication; medium refactor benefit

Sources: [`transfer_edge_appearance`](../../../crates/cadmpeg-codec-freecad/src/gui.rs:909), [`transfer_vertex_appearance`](../../../crates/cadmpeg-codec-freecad/src/gui.rs:976).

**Evidence.** The two functions repeat payload-prefix selection, provider identity handling, packed RGBA conversion, nonnegative size filtering, appearance construction, and binding creation.

**Effect.** Provider/style behavior can drift between edge and vertex presentation. Target type, style property, and labels are legitimate differences.

**Repair direction.** Share style/binding construction around explicit appearance targets and metadata; preserve the domain-specific target selection.

### FREECAD2-05 — Text B-rep table readers repeat their envelope parser

P3 · Duplicate / forwarding layer · Confidence: High

Sources: [`parse_curve2ds`](../../../crates/cadmpeg-codec-freecad/src/brep.rs:3707), [`parse_curves`](../../../crates/cadmpeg-codec-freecad/src/brep.rs:4963), [`parse_surfaces`](../../../crates/cadmpeg-codec-freecad/src/brep.rs:4535).

**Evidence.** All three find start/end table tokens, take the declared count, bound minimum token consumption, parse indexed rows, and reject trailing tokens. Only section labels and row parsers differ.

**Effect.** The same framing, allocation, and trailing-data rules are maintained three times.

**Repair direction.** Share the table envelope reader and pass the owned row parser; keep curve and surface grammar code separate.

## cadmpeg-codec-iges

### IGES2-01 — Wrong Bezier spans can certify unequal surface boundaries

P1 · Numerical · Confidence: High

Sources: [`homogeneous_bezier_spans`](../../../crates/cadmpeg-codec-iges/src/entities/surfaces.rs:436), [`homogeneous_curve_boundary_matches`](../../../crates/cadmpeg-codec-iges/src/entities/surfaces.rs:914), [`surface_boundary_is_closed`](../../../crates/cadmpeg-codec-iges/src/entities/surfaces.rs:992).

**Evidence.** The same unclamped and discontinuous examples as IR2-01 produce wrong spans. Two degree-2 curves with full internal multiplicity that differ only at their final control point are reported equal at resolution 0; their separation at t=1.5 is 1.

**Effect.** The Type 128 closed-surface flag check can accept inconsistent boundaries. The same decomposition also feeds aligned rails for ruled surfaces. This duplicate algorithm has already drifted from the corrected Rhino implementation.

**Repair direction.** Use one corrected shared Bezier extraction algorithm; test both active endpoint clipping and degree+1 interior multiplicity.

**Probe:** `iges_spans::unclamped; iges_spans::discontinuous; iges_closure::omitted_control`.

### IGES2-02 — Boundary equality underflows after multiplying homogeneous weights

P1 · Numerical · Confidence: High

Sources: [`homogeneous_curve_boundary_matches`](../../../crates/cadmpeg-codec-iges/src/entities/surfaces.rs:914).

**Evidence.** Two parallel unit lines separated by 2, both weighted [1e-200,1e-200], compare equal at resolution 1e-6. Both cross-products and the threshold underflow to zero.

**Effect.** Closure admission can accept genuinely open boundaries with ordinary coordinates. Normalizing the common weight scale must not change the answer.

**Repair direction.** Scale the homogeneous operands before cross-products and compare a scale-safe residual against the resolution.

**Probe:** `iges_closure::small_weights`.

## cadmpeg-codec-nx

### NX2-01 — JT transform parsing rejects finite orthogonal scales through f32 norm overflow

P2 · Numerical · Confidence: High

Sources: [`parse_jt9_geometric_transform_body`](../../../crates/cadmpeg-codec-nx/src/native/display_jt.rs:2005).

**Evidence.** Byte fixtures differing only in uniform diagonal scale accept 1 but reject 1e20 and 1e-30. Squaring finite f32 rows produces infinity or zero.

**Effect.** Valid finite transform carriers become unavailable to the JT presentation path. This is an extreme-scale parser limitation.

**Repair direction.** Promote the norm/rank arithmetic or scale the rows before computing lengths and orthogonality.

**Probe:** `nx::f32_scale`.

### NX2-02 — Linked and target index row projection duplicate source resolution

P3 · Duplicate / forwarding layer · Confidence: High duplication; medium refactor benefit

Sources: [`data_block_linked_index_rows`](../../../crates/cadmpeg-codec-nx/src/native/om/column_row.rs:125), [`data_block_target_index_rows`](../../../crates/cadmpeg-codec-nx/src/native/om/column_row.rs:173).

**Evidence.** Both traverse indexed sections, derive storage/base offsets, locate the opening block, absolutize the frame, resolve control-index references, assign row ordinals, and collect records. Scanner and output wrapper differ.

**Effect.** Offset/provenance and reference admission changes must be mirrored across both paths.

**Repair direction.** Share the section/frame projection flow with explicit row construction; retain distinct row scanners and wire types.

### NX2-03 — Class and field registry collection duplicate merge policy

P3 · Duplicate / forwarding layer · Confidence: High duplication; medium refactor benefit

Sources: [`class_definitions`](../../../crates/cadmpeg-codec-nx/src/native/om.rs:2924), [`field_definitions`](../../../crates/cadmpeg-codec-nx/src/native/om.rs:2988).

**Evidence.** Both independently merge framed and indexed definitions into a BTreeMap keyed by entry/offset, prefer framed records, and construct the same ordinal/provenance/tail fields.

**Effect.** Source precedence, identity, and registry-tail handling have duplicate owners.

**Repair direction.** Share the registry collection and precedence operation. Keep class/field record types and labels explicit.

## cadmpeg-codec-rhino

### RHINO2-01 — Legacy B-rep vertex averaging can create infinity from finite endpoints

P2 · Numerical · Confidence: High

Sources: [`vertex.point_sum[0] +=`](../../../crates/cadmpeg-codec-rhino/src/brep.rs:1323), [`accumulated.vertex.point = Point3`](../../../crates/cadmpeg-codec-rhino/src/brep.rs:1366).

**Evidence.** The current accumulator and final mean expressions turn two identical endpoints (1e308,0,0) into (infinity,0,0). Their exact mean is the same finite endpoint.

**Effect.** Legacy topology assembly creates a nonfinite point before transfer. The source arithmetic is reproduced; a complete legacy archive fixture was not constructed.

**Repair direction.** Use scale-safe averaging or scaled sums, including mixed-sign inputs; retain source tolerances and finite admission.

**Probe:** `rhino_mean::finite_endpoint_average`.

### RHINO2-02 — Optional localizer curve and surface readers duplicate child framing

P3 · Duplicate / forwarding layer · Confidence: High duplication; medium refactor benefit

Sources: [`optional_curve`](../../../crates/cadmpeg-codec-rhino/src/morph.rs:189), [`optional_surface`](../../../crates/cadmpeg-codec-rhino/src/morph.rs:217).

**Evidence.** Both read an anonymous child, apply the same version gate, read a presence boolean, parse the payload, skip trailing child bytes, and advance the parent. Labels and the payload parser differ.

**Effect.** Child-boundary and version handling have two copies. The NURBS payload parsers themselves are distinct.

**Repair direction.** Use one private optional-child reader taking the owned payload parser and diagnostic label.

## cadmpeg-codec-sldprt

### SLDPRT2-01 — Arc tessellation reports zero sagitta for a nonzero chord error

P2 · Numerical · Confidence: High

Sources: [`planar_arc_segments`](../../../crates/cadmpeg-codec-sldprt/src/tessellation.rs:1664).

**Evidence.** For span 1e-5, radius 1e12, and requested tolerance 1e-9, the cap is 4096 segments. Reported error is 0, while 2*r*sin(span/(4*n))^2 gives 7.4505805969e-7.

**Effect.** Downstream error accounting underestimates the actual approximation error. The witness is a very large-radius, short-angle arc; no ordinary-radius corpus frequency is claimed.

**Repair direction.** Compute sagitta through the stable sine-squared identity, use a stable inverse for segment selection, and propagate the actual error when the cap is reached.

**Probe:** `sld::sagitta`.

### SLDPRT2-02 — Seeded surface projection wrapper adds no contract

P3 · Duplicate / forwarding layer · Confidence: High

Sources: [`nurbs_seeded_surface_projection`](../../../crates/cadmpeg-codec-sldprt/src/brep/graph.rs:4978).

**Evidence.** The wrapper forwards all three arguments unchanged to nurbs_surface_parameter_near_point and returns its result unchanged. Caller residual checks remain outside it.

**Effect.** The extra name hides the true numerical owner and adds an unnecessary maintenance step.

**Repair direction.** Remove the wrapper and have its callers use the owning IR function directly.

## cadmpeg-codec-step

### STEP2-01 — Mesh volume and centroid depend on translation

P2 · Numerical · Confidence: High

Sources: [`mesh_properties`](../../../crates/cadmpeg-codec-step/src/reader/validation.rs:330).

**Evidence.** A closed tetrahedron with vertices (0,0,0),(2,0,0),(0,1,0),(0,0,1) has volume 1/3 and centroid (0.5,0.25,0.25). At translation (1e6,1e6,1e6), the routine switches to an area centroid with relative x=0.5833333333. At translation 1e9 it reports volume about 666666666.7.

**Effect.** STEP validation-property comparisons can report false mismatches for the same solid moved in model space. The origin-based triple products cancel; the absolute-coordinate volume epsilon also changes centroid selection.

**Repair direction.** Compute moments relative to a local reference point and restore the translation afterward; base degeneracy thresholds on local extent, with stable accumulation.

**Probe:** `step::translation`.

## cadmpeg-ir

### IR2-01 — Bezier patch decomposition changes valid splines

P1 · Numerical · Confidence: High

Sources: [`homogeneous_bezier_spans`](../../../crates/cadmpeg-ir/src/eval.rs:269), [`rational_surface_patches_with_budget`](../../../crates/cadmpeg-ir/src/eval.rs:325).

**Evidence.** For degree 2, knots [-1,-1,0,1,2,2] and poles (0,0), (1,1), (2,0), the decomposed midpoint has y=0.5; the spline has y=0.75. With knots [0,0,0,1,1,1,2,2,2] and x poles 0..5, the second span evaluates to x=3 instead of 4.

**Effect.** Surface inversion and bounds consume patches that no longer represent the source surface. Endpoint clamping is missing; interval_index*degree also selects the wrong controls after full-multiplicity internal knots. The probe establishes patch corruption, not a complete codec conversion failure.

**Repair direction.** Extract spans by their actual knot/control indices, including active endpoint insertion. Share the corrected algorithm with IGES; its copy has the same defects.

**Probe:** `ir_spans::unclamped; ir_spans::discontinuous`.

### IR2-02 — Surface Newton solver stops on a regular small plane

P2 · Numerical · Confidence: High

Sources: [`nurbs_surface_parameter_near_point`](../../../crates/cadmpeg-ir/src/eval.rs:1280).

**Evidence.** A 1e-5 by 1e-5 bilinear plane, target (0.3e-5,0.4e-5,0), and seed (0,0) return (0,0), although one Newton step gives (0.3,0.4). The determinant is 1e-20 and is rejected against absolute machine epsilon.

**Effect.** SolidWorks projection callers forward-check the candidate and can discard valid geometry. The API documents a candidate, so this is failed convergence, not a falsely claimed nearest-point certificate. Its separate absolute squared-distance stopping gate also prevents scale-independent accuracy.

**Repair direction.** Scale the least-squares columns and use a relative rank test. Use a residual/parameter convergence contract that does not impose an undocumented absolute model-space floor.

**Probe:** `ir_surface::small_plane`.

### IR2-03 — Rational search speed bounds depend on common weight scale

P2 · Numerical · Confidence: High

Sources: [`nurbs_curve_speed_bound_about`](../../../crates/cadmpeg-ir/src/eval.rs:1780), [`nurbs_curve_parameter_near_point`](../../../crates/cadmpeg-ir/src/eval.rs:1622), [`nurbs_pcurve_contains_point`](../../../crates/cadmpeg-ir/src/eval.rs:2203).

**Evidence.** The same unit line has speed bound Some(1) with weights [1,1], but None with [1e-200,1e-200]. minimum_weight.powi(2) underflows, producing 0/0. The pcurve containment path repeats this formula.

**Effect.** A geometrically unchanged curve loses parameter-witness and containment support before search starts. This is outside the point/tangent evaluators fixed in the previous audit.

**Repair direction.** Normalize common weight scale or evaluate the bound through scale-safe ratios/products; retain a conservative upper bound.

**Probe:** `ir_speed::common_weights`.

### IR2-04 — Periodic parameter facade still returns Some(NaN)

P2 · Numerical · Confidence: High

Sources: [`map_nurbs_curve_parameter`](../../../crates/cadmpeg-ir/src/eval.rs:1957).

**Evidence.** A periodic curve with domain [-1e308,-9e307] and finite parameter 1e308 returns Some(NaN): parameter-lower overflows before rem_euclid.

**Effect.** Tangent, second-derivative, and model differential paths call this facade. The earlier periodic_parameter fix did not reach this separate implementation.

**Repair direction.** Use the existing scale-safe periodic mapping implementation, and require a finite in-domain result.

**Probe:** `ir_phase::finite_phase`.

### IR2-05 — Curve witness accepts a point outside zero tolerance

P2 · Numerical · Confidence: High

Sources: [`nurbs_curve_parameter_near_point`](../../../crates/cadmpeg-ir/src/eval.rs:1622), [`nearest_boundary_witness`](../../../crates/cadmpeg-ir/src/eval.rs:1914).

**Evidence.** For a unit X-axis line and point (0,1e-200,0), tolerance 0 returns the endpoint witness 0. The current squared-distance closure underflows to zero; the actual distance is 1e-200.

**Effect.** The documented forward-residual guarantee is false for very small separations. This is an extreme-scale boundary case, not evidence of a problem at ordinary positive CAD tolerances.

**Repair direction.** Use the shared scale-safe point distance in the boundary and interval residual checks.

**Probe:** `ir_witness::false_zero_residual; ir_surface::cached_public_curve_residual`.

### IR2-06 — Sweep-law quotient differentiation overflows avoidable intermediates

P2 · Numerical · Confidence: High

Sources: [`scalar_sweep_law_differential`](../../../crates/cadmpeg-ir/src/eval.rs:5027).

**Evidence.** At X=1, X/1e200 has derivative 0 instead of 1e-200. X/1e-200 returns None although its value and derivative, 1e200, are finite. The quotient rule squares the denominator.

**Effect.** Valid law-driven sweep derivatives become zero or unavailable. The affected function feeds sweep surface differential evaluation.

**Repair direction.** Evaluate the quotient derivative using scale-safe ratios/products instead of an unscaled squared denominator.

**Probe:** `ir_laws::quotient`.

### IR2-07 — Hyperbolic sweep laws lose finite values and derivatives

P2 · Numerical · Confidence: High

Sources: [`scalar_unary_sweep_law_differential`](../../../crates/cadmpeg-ir/src/eval.rs:5108).

**Evidence.** TANH at value 20 and incoming derivative 1e20 returns derivative 0; the stable result is about 1699.341702. ARCSINH at value/derivative 1e200 returns derivative 0 instead of approximately 1. ARCOTH(1e20) returns value 0 instead of approximately 1e-20.

**Effect.** Cancellation in 1-tanh(x)^2 and (x+1)/(x-1), plus overflowing x*x, damages law values and chain-rule derivatives. These are one family in the same dispatch function.

**Repair direction.** Use stable hyperbolic identities, hypot-based denominators, and log1p/atanh formulations with their domain checks.

**Probe:** `ir_laws::hyperbolic`.

### IR2-08 — Helix circular-frame admission still uses an unsafe norm

P2 · Numerical · Confidence: High

Sources: [`impl HelixPathConstruction`](../../../crates/cadmpeg-ir/src/geometry.rs:1933).

**Evidence.** The current admission expressions accept orthogonal equal-radius vectors at radius 1, but reject them at finite radii 1e200 and 1e-200. Squared lengths overflow or underflow before the radius comparison.

**Effect.** Finite circular helix constructions are refused even though the shared Vector3 norm now handles these scales. This is an extreme-scale admission defect.

**Repair direction.** Use the shared norm for both radii and preserve the existing acceptance tolerance.

**Probe:** `ir_helix::finite_circular_frame`.
