# Third numerical and duplicate audit — 2026-09-20

33 new ranked entries: 22 numerical defect families and 11 consolidation candidates. All 21 crate directories were screened. No crate reached the requested limit of 80; no findings were omitted to meet it.

This document records the original findings. All 33 repairs and their verification are recorded in [the resolution report](resolutions.md). Items already resolved by the [first audit](../2026-09-20-numerical-duplicates/resolutions.md) and [second audit](../2026-09-20-numerical-duplicates-followup/resolutions.md) are excluded. A separate remaining copy or owner is identified when a related problem was repaired previously.

Within each crate, correctness findings precede consolidation candidates. P2 means a demonstrated correctness issue with a material path or a parameter-scale trigger; P3 means a rare finite-range trigger or a maintenance opportunity. These are relative priorities, not a release-blocking determination. Duplicate entries do not claim a current output defect.

The highest-value repairs are CATIA surface membership, the three Creo sketch-intersection routines, STEP similarity admission, and SolidWorks sketch coordinate keys. The CATIA witness uses a unit-size plane: the failure depends on parameterization, not extreme model coordinates.

| Crate | Numerical | Consolidation | Total |
|---|---:|---:|---:|
| `cadmpeg` | 0 | 0 | 0 |
| `cadmpeg-asm` | 0 | 1 | 1 |
| `cadmpeg-codec-catia` | 2 | 0 | 2 |
| `cadmpeg-codec-creo` | 4 | 2 | 6 |
| `cadmpeg-codec-f3d` | 3 | 2 | 5 |
| `cadmpeg-codec-freecad` | 0 | 0 | 0 |
| `cadmpeg-codec-iges` | 1 | 0 | 1 |
| `cadmpeg-codec-inventor` | 1 | 0 | 1 |
| `cadmpeg-codec-nx` | 3 | 2 | 5 |
| `cadmpeg-codec-rhino` | 2 | 1 | 3 |
| `cadmpeg-codec-sat` | 0 | 0 | 0 |
| `cadmpeg-codec-sldprt` | 3 | 1 | 4 |
| `cadmpeg-codec-step` | 1 | 1 | 2 |
| `cadmpeg-container` | 0 | 0 | 0 |
| `cadmpeg-core` | 0 | 0 | 0 |
| `cadmpeg-fuzz` | 0 | 0 | 0 |
| `cadmpeg-ir` | 2 | 1 | 3 |
| `cadmpeg-parasolid` | 0 | 0 | 0 |
| `cadmpeg-protein` | 0 | 0 | 0 |
| `cadmpeg-registry` | 0 | 0 | 0 |
| `cadmpeg-test-support` | 0 | 0 | 0 |

The [coverage and disposition notes](coverage.md) state the screening limits and explain unranked observations. The [machine-readable findings](findings.json) contain locations, source hashes, evidence and repair direction.

## Findings

### cadmpeg-asm

#### ASM3-01 · P3 · Curve and surface cache scans duplicate ambiguity handling

**duplicate; source-reviewed.** [cadmpeg-asm/src/nurbs/core.rs:465](../../../crates/cadmpeg-asm/src/nurbs/core.rs#L465); [cadmpeg-asm/src/nurbs/core.rs:519](../../../crates/cadmpeg-asm/src/nurbs/core.rs#L519)

Both enumerate integer widths and marker positions, decode a candidate and refuse a second successful candidate. The payload parser differs; marker enumeration and uniqueness policy are the same. This is not a request to merge the different curve/surface wire grammars.

Evidence: Complete scan bodies compared; existing block decoders remain the owning parsers.

Repair direction: Share the unique cache-candidate scan with a decoding callback and remove the duplicated control flow.

### cadmpeg-codec-catia

#### CATIA3-01 · P2 · Surface membership depends on parameter scale and misses an exact planar point

**numerical; reproduced.** [cadmpeg-codec-catia/src/families/standard/decode.rs:6867](../../../crates/cadmpeg-codec-catia/src/families/standard/decode.rs#L6867); [cadmpeg-codec-catia/src/families/standard/decode.rs:6919](../../../crates/cadmpeg-codec-catia/src/families/standard/decode.rs#L6919)

A 1-by-1 plane with U domain [0,1e-10] and V domain [0,1] contains (0.3,0.4,0) at UV (3e-11,0.4). All refinement seeds stop at the determinant gate; the best squared residual is 0.05, above the 0.002 membership tolerance squared. The raw normal equations compare the determinant with the square of the largest column norm. This misclassifies an independent, anisotropic basis as singular. The shared IR inverse was repaired previously; this separate CATIA solver remains. Failure is reported as unknown membership, not proof that the point is off the surface.

Evidence: catia::anisotropic_surface; full start-grid/witness path extracted.

Repair direction: Reuse the shared scaled least-squares solver and retain CATIA’s search budget and positive-witness policy.

#### CATIA3-02 · P3 · Planar chart fitting loses an exact rigid map when covariance overflows

**numerical; reproduced.** [cadmpeg-codec-catia/src/families/freeform/mod.rs:2664](../../../crates/cadmpeg-codec-catia/src/families/freeform/mod.rs#L2664)

Four symmetric plane sites at (+/-a,0) and (0,+/-a), with identical target loci, have a unique identity chart. The solver accepts a=1 and refuses a=1e200. Its raw covariance sums overflow before the rotation is normalized. The target-plane checks and coordinate values remain finite. This can prevent an otherwise exact consolidated pcurve rechart.

Evidence: catia_chart::identity_rechart; unchanged fitting function with minimal rigid-chart record scaffolding.

Repair direction: Scale centered sites before accumulating covariance, and preserve uniqueness, reflection handling and the source residual allowance.

### cadmpeg-codec-creo

#### CREO3-01 · P2 · Sketch line-circle intersection invents a tangent for a missed circle

**numerical; reproduced.** [cadmpeg-codec-creo/src/decode/sketch/intersect.rs:95](../../../crates/cadmpeg-codec-creo/src/decode/sketch/intersect.rs#L95)

For radius 1e-6 at the origin and segment (-1e-6,2e-6) to (1e-6,2e-6), the function returns (0,2e-6). The line misses the circle by 1e-6. A squared-radius tolerance uses max(1), then clamps a negative radial remainder to zero. This is the sketch path, not the previously repaired trim helper.

Evidence: creo::missed_circle; caller intersect_section_carriers and incident-carrier reconciliation inspected.

Repair direction: Scale the geometry before solving; a returned candidate must satisfy the radial residual in the intended length tolerance.

#### CREO3-02 · P2 · Sketch circle-circle solver reports a nontangent midpoint as a tangent

**numerical; reproduced.** [cadmpeg-codec-creo/src/decode/sketch/intersect.rs:168](../../../crates/cadmpeg-codec-creo/src/decode/sketch/intersect.rs#L168)

Two circles of radius 1e-6 with centers (0,0) and (1e-6,0) have two intersections. The function instead returns (5e-7,0), which is on neither circle. The max(1) squared-height allowance overwhelms the small geometry. The prior trim_circle_circle_intersection fix did not reach this copy.

Evidence: creo::nontangent_circles.

Repair direction: Share the existing scale-safe circle tangency logic where contracts match, and check the candidate on both carriers.

#### CREO3-03 · P2 · Perpendicular sketch lines are rejected because of their segment scale

**numerical; reproduced.** [cadmpeg-codec-creo/src/decode/sketch/intersect.rs:50](../../../crates/cadmpeg-codec-creo/src/decode/sketch/intersect.rs#L50)

Perpendicular segments from (-1e-7,0) to (1e-7,0) and (0,-1e-7) to (0,1e-7) have the unique carrier intersection (0,0). The determinant is rejected as parallel because the direction scale is floored at 1. The angle is well conditioned; changing line representation length should not change carrier intersection existence.

Evidence: creo::small_perpendicular_lines.

Repair direction: Normalize or independently scale the two directions before the angular singularity test and solve relative to an origin.

#### CREO3-04 · P3 · Separate profile line-arc and arc-arc predicates lose finite intersections

**numerical; reproduced.** [cadmpeg-codec-creo/src/decode/sweep/profiles.rs:620](../../../crates/cadmpeg-codec-creo/src/decode/sweep/profiles.rs#L620); [cadmpeg-codec-creo/src/decode/sweep/profiles.rs:650](../../../crates/cadmpeg-codec-creo/src/decode/sweep/profiles.rs#L650)

For a circle of radius 1e200, a diameter segment with endpoints +/-2e200 is reported not to intersect it. Two equal circles of that radius separated by one radius are also reported disjoint. Both have two finite intersections. Raw quadratic/discriminant and squared-radius intermediates overflow. These sweep-profile predicates are separate from both the repaired trim helpers and the sketch-carrier routines ranked above.

Evidence: creo_profile::large_intersections extracts both predicates and their final point-on-arc gate.

Repair direction: Share scale-safe intersection geometry while retaining profile angle and tolerance policies; check the final candidate residuals.

#### CREO3-05 · P3 · Cross-section aggregation repeats the same selection, relocation and sorting policy

**duplicate; source-reviewed.** [cadmpeg-codec-creo/src/container.rs:1285](../../../crates/cadmpeg-codec-creo/src/container.rs#L1285); [cadmpeg-codec-creo/src/container.rs:1529](../../../crates/cadmpeg-codec-creo/src/container.rs#L1529); [cadmpeg-codec-creo/src/container.rs:1707](../../../crates/cadmpeg-codec-creo/src/container.rs#L1707)

Three paths select Xsections, require Sld_Xsections, decode rows, add the section offset and sort by absolute offset. The row decoders differ; section selection and coordinate relocation do not. Other paired section aggregators have the same owner.

Evidence: Normalized-body census plus full function comparison.

Repair direction: Use one section traversal/collection owner with decoder and relocation callbacks. Keep record-specific parsers separate; do not add a trait solely for an offset field.

#### CREO3-06 · P3 · Replay consensus policy has three independent implementations

**duplicate; source-reviewed.** [cadmpeg-codec-creo/src/decode/feature_history/dependencies.rs:178](../../../crates/cadmpeg-codec-creo/src/decode/feature_history/dependencies.rs#L178); [cadmpeg-codec-creo/src/decode/feature_history/dependencies.rs:360](../../../crates/cadmpeg-codec-creo/src/decode/feature_history/dependencies.rs#L360); [cadmpeg-codec-creo/src/decode/feature_history/dependencies.rs:373](../../../crates/cadmpeg-codec-creo/src/decode/feature_history/dependencies.rs#L373)

All three filter records by feature id, require a first match, and require all selected ID slices to be equal. Two even use the same record type. This duplicates the ambiguity/refusal policy.

Evidence: Normalized-body comparison, including the selected ID field in each case.

Repair direction: Centralize consensus over an iterator of ID slices and retain the typed field selection at callers.

### cadmpeg-codec-f3d

#### F3D3-01 · P3 · Segment intersection multiplies signs and loses separation to underflow

**numerical; reproduced.** [cadmpeg-codec-f3d/src/design/geometry.rs:2201](../../../crates/cadmpeg-codec-f3d/src/design/geometry.rs#L2201)

Parallel segments (0,0)–(1e-100,0) and (0,2e-100)–(1e-100,2e-100) are reported as intersecting. Their nonzero orientations survive, but orientation products underflow to zero and satisfy <= 0. The collinear bounding-box branch is skipped. This predicate feeds profile-boundary intersection.

Evidence: f3d::separated_segments.

Repair direction: Compare orientation signs without multiplying them; separately make determinant evaluation scale-safe.

#### F3D3-02 · P3 · Both remaining arc-arc solvers lose finite intersections at extreme scales

**numerical; reproduced.** [cadmpeg-codec-f3d/src/design/geometry.rs:747](../../../crates/cadmpeg-codec-f3d/src/design/geometry.rs#L747); [cadmpeg-codec-f3d/src/design/geometry.rs:2277](../../../crates/cadmpeg-codec-f3d/src/design/geometry.rs#L2277)

Two full circles with radius r and centers separated by r have two intersections. At r=1e-200 the point solver treats the centers as coincident and returns None; at r=1e200 it returns an empty point list. Both arc-arc implementations retain raw squared center distances and radii. The earlier line-arc consolidation does not cover this family.

Evidence: f3d::crossing_circles exercises the point solver; boolean copy inspected.

Repair direction: Consolidate the circle geometry computation with scaled arithmetic, retaining the distinct coincident-arc overlap contract.

#### F3D3-03 · P3 · Profile distance helper loses finite distances and duplicates shared math

**numerical; reproduced.** [cadmpeg-codec-f3d/src/design/geometry.rs:2621](../../../crates/cadmpeg-codec-f3d/src/design/geometry.rs#L2621); [cadmpeg-codec-f3d/src/design/geometry.rs:2171](../../../crates/cadmpeg-codec-f3d/src/design/geometry.rs#L2171)

The local point_distance returns 0 for a distance of 1e-200 and infinity for 1e200. It is used for arrangement and arc membership. Segment distance additionally forms raw squared lengths for projection. The latter needs scaled projection, so replacing only the final square root does not repair that path. Corpus occurrence and a complete profile-decode failure are not measured.

Evidence: f3d::distance_range; segment projection and callers inspected.

Repair direction: Use hypot for 2D distance and scaled dot/projection arithmetic for segment distance.

#### F3D3-04 · P3 · Sketch point and text decoders copy bulk-stream traversal

**duplicate; source-reviewed.** [cadmpeg-codec-f3d/src/design/decode/sketch.rs:1505](../../../crates/cadmpeg-codec-f3d/src/design/decode/sketch.rs#L1505); [cadmpeg-codec-f3d/src/design/decode/sketch.rs:1598](../../../crates/cadmpeg-codec-f3d/src/design/decode/sketch.rs#L1598)

Both select design Bulkstream entries, fetch entry bytes and associated metadata, skip missing metadata, then extend decoded output. Only the row decoder differs. This is separate from the canvas/decal scope traversal already consolidated.

Evidence: Normalized-body census and complete function comparison.

Repair direction: Put stream selection and metadata association in one owner and call the point/text decoders through it.

#### F3D3-05 · P3 · Timestamp lookup maintains parallel maps and repeated target dispatch

**duplicate; source-reviewed.** [cadmpeg-codec-f3d/src/writer/generate/attributes.rs:24](../../../crates/cadmpeg-codec-f3d/src/writer/generate/attributes.rs#L24); [cadmpeg-codec-f3d/src/writer/generate/attributes.rs:257](../../../crates/cadmpeg-codec-f3d/src/writer/generate/attributes.rs#L257); [cadmpeg-codec-f3d/src/writer/generate/attributes.rs:272](../../../crates/cadmpeg-codec-f3d/src/writer/generate/attributes.rs#L272)

Five timestamp maps and five ordinal maps repeat Body/Face/Edge/Coedge/Vertex dispatch. The ordinals are assigned later in target-model order, so they are not the native timestamp’s original ordinal. A combined entry can retain the timestamp and its optional generated ordinal. No current mismatch was demonstrated.

Evidence: Struct, constructor, split_timestamp_ordinals macro and both lookup methods reviewed.

Repair direction: Store the generated ordinal with its timestamp entry and share the target lookup; preserve first-source-wins and model-order assignment.

### cadmpeg-codec-iges

#### IGES3-01 · P3 · B-rep endpoint distance copy can erase a real gap

**numerical; reproduced.** [cadmpeg-codec-iges/src/entities/evaluation.rs:266](../../../crates/cadmpeg-codec-iges/src/entities/evaluation.rs#L266); [cadmpeg-codec-iges/src/entities/brep.rs:153](../../../crates/cadmpeg-codec-iges/src/entities/brep.rs#L153); [cadmpeg-codec-iges/src/entities/csg.rs:49](../../../crates/cadmpeg-codec-iges/src/entities/csg.rs#L49)

The local sum-of-squares distance returns 0 for a 1e-200 gap and infinity for finite distance 1e200. B-rep endpoint/chain checks and CSG closure call this helper. A sufficiently small positive source tolerance can therefore accept a gap larger than the tolerance. Existing Point3::distance already implements robust distance.

Evidence: iges::residual_range plus call-site review; no complete IGES file fixture.

Repair direction: Remove the private arithmetic copy and use Point3::distance at its callers.

### cadmpeg-codec-inventor

#### INVENTOR3-01 · P3 · Line-carrier agreement rejects finite, exactly collinear data

**numerical; reproduced.** [cadmpeg-codec-inventor/src/sketch.rs:1593](../../../crates/cadmpeg-codec-inventor/src/sketch.rs#L1593); [cadmpeg-codec-inventor/src/sketch.rs:680](../../../crates/cadmpeg-codec-inventor/src/sketch.rs#L680)

Origin (0,0), direction (1e200,1e200), start (1e200,1e200), and end (2e200,2e200) are exactly collinear. The comparison returns false because cross products and norm products overflow. parse_line admits finite components without requiring a unit direction, so that precondition does not rule out the trigger. project_geometry then omits the solved line.

Evidence: inventor::finite_parallel; native parser and projection caller reviewed.

Repair direction: Compare normalized directions and use a scaled point-to-carrier residual. Preserve the existing relative agreement policy.

### cadmpeg-codec-nx

#### NX3-01 · P2 · Intersection tangent nullspace depends on arbitrary derivative scale

**numerical; reproduced.** [cadmpeg-codec-nx/src/decode/offset.rs:2032](../../../crates/cadmpeg-codec-nx/src/decode/offset.rs#L2032); [cadmpeg-codec-nx/src/decode/offset.rs:1907](../../../crates/cadmpeg-codec-nx/src/decode/offset.rs#L1907)

A regular two-plane Jacobian with rows [a,0,-a,0], [0,a,0,0], [0,0,0,-a] has the same normalized null vector for every nonzero a. The helper succeeds at a=1 and fails at a=1e-5 and 1e100. Cofactors are cubically scaled, followed by a squared norm and absolute 1e-14 cutoff. The caller has a chord/least-squares fallback; that can rescue some inputs, so this probe does not prove that every affected surface intersection fails.

Evidence: nx::tangent_scale; fallback and Jacobian construction reviewed.

Repair direction: Scale the nullspace computation before cofactors and use a robust norm. Apply relative rank criteria and preserve the independent fallback.

#### NX3-02 · P3 · Periodic lifting can overflow despite a finite nearest representative

**numerical; reproduced.** [cadmpeg-codec-nx/src/decode/offset.rs:1761](../../../crates/cadmpeg-codec-nx/src/decode/offset.rs#L1761); [cadmpeg-codec-nx/src/decode/blend.rs:2666](../../../crates/cadmpeg-codec-nx/src/decode/blend.rs#L2666)

Lifting value -1e308 near reference 1e308 with period 1e307 returns infinity because reference-value overflows. A finite representative exists near the reference. The blend list helper contains the same subtract/divide/round/multiply construction. This is separate from the repaired IR periodic wrap.

Evidence: nx::finite_phase; blend copy inspected.

Repair direction: Use a shared range-safe phase-lifting operation that reduces phases before subtracting and checks the final result.

#### NX3-03 · P3 · Intersection endpoint checks retain an underflowing distance copy

**numerical; reproduced.** [cadmpeg-codec-nx/src/intersection.rs:1117](../../../crates/cadmpeg-codec-nx/src/intersection.rs#L1117); [cadmpeg-codec-nx/src/intersection.rs:516](../../../crates/cadmpeg-codec-nx/src/intersection.rs#L516)

The helper returns zero for a 1e-200 endpoint gap and infinity for finite distance 1e200. enrich compares it directly with the chart fit tolerance when accepting stored endpoint terms and selecting topology endpoint permutations. Shared robust Point3::distance already exists.

Evidence: nx_distance::residual_range and enrich call-site review.

Repair direction: Remove the local distance implementation and use the shared point method.

#### NX3-04 · P3 · Labeled and unlabeled operation visitors repeat section resolution

**duplicate; source-reviewed.** [cadmpeg-codec-nx/src/native/features.rs:3621](../../../crates/cadmpeg-codec-nx/src/native/features.rs#L3621); [cadmpeg-codec-nx/src/native/features.rs:3657](../../../crates/cadmpeg-codec-nx/src/native/features.rs#L3657)

Both resolve feature-history section links against OM sections, calculate the same absolute offset/key and then traverse records. The record iterator and record type differ. Repeated section-link resolution is an independent shared responsibility.

Evidence: Full functions compared after normalized-body census.

Repair direction: Share the resolved-section traversal; keep labeled and unlabeled record iteration typed at the caller.

#### NX3-05 · P3 · Output-free feature predicates duplicate the body-reference policy

**duplicate; source-reviewed.** [cadmpeg-codec-nx/src/decode/feature_completeness.rs:71](../../../crates/cadmpeg-codec-nx/src/decode/feature_completeness.rs#L71); [cadmpeg-codec-nx/src/decode/feature_completeness.rs:91](../../../crates/cadmpeg-codec-nx/src/decode/feature_completeness.rs#L91)

The two predicates repeat empty-output checks and five exact/prefix source-property checks. Only the operation-kind match differs. Adding a new source body-reference spelling can change one feature’s admission while leaving the other stale. No present behavioral disagreement was found.

Evidence: Complete predicate comparison.

Repair direction: Put absence of outputs/body-reference evidence in one predicate and keep operation-kind matching explicit.

### cadmpeg-codec-rhino

#### RHINO3-01 · P3 · Plane parameter mapping breaks even an identity map at finite extremes

**numerical; reproduced.** [cadmpeg-codec-rhino/src/surfaces.rs:971](../../../crates/cadmpeg-codec-rhino/src/surfaces.rs#L971); [cadmpeg-codec-rhino/src/surfaces.rs:114](../../../crates/cadmpeg-codec-rhino/src/surfaces.rs#L114)

With domain and physical extents both [0,a], mapping endpoint a returns 0 at a=1e-200 and infinity at a=1e200. The intermediate product is formed before division. The helper maps plane-surface pcurve coordinates; it is separate from curves::remap_nurbs_domain repaired in the first audit.

Evidence: rhino::plane_parameter_range; PlaneParameterization::map_point reviewed.

Repair direction: Reuse bounded affine interpolation with exact endpoints and scale-safe interval ratios.

#### RHINO3-02 · P3 · Knot periodicity test accepts unequal spacing when its tolerance overflows

**numerical; reproduced.** [cadmpeg-codec-rhino/src/surfaces.rs:1066](../../../crates/cadmpeg-codec-rhino/src/surfaces.rs#L1066)

For stored knots [-1e308,-1e308,-9e307,9e307,1e308,1e308], order 3 and 5 control vertices, paired spacings 0 and 1e307 differ. The function reports periodic because its domain-width tolerance becomes infinity. The reader’s stored-domain guard requires increasing finite knots but does not require their subtraction to be finite. Writer round-trip admission also uses this predicate.

Evidence: rhino::nonperiodic_knots; read_knots, validate_stored_domain, reader and writer callers inspected.

Repair direction: Compare scaled knot differences and form the relative allowance without overflowing the domain subtraction.

#### RHINO3-03 · P3 · Legacy NURBS object wrappers duplicate the same chunk envelope

**duplicate; source-reviewed.** [cadmpeg-codec-rhino/src/legacy.rs:1039](../../../crates/cadmpeg-codec-rhino/src/legacy.rs#L1039); [cadmpeg-codec-rhino/src/legacy.rs:1050](../../../crates/cadmpeg-codec-rhino/src/legacy.rs#L1050)

Both construct an identical bounded reader, require the same RHINOIO object-data chunk, call a data decoder and skip the remaining envelope. Only the data decoder/result type differs. This is distinct from the repaired optional morph-localizer envelope.

Evidence: Complete wrapper bodies compared.

Repair direction: Keep one legacy object-data envelope helper with a typed decoding callback and remove the duplicate wrappers where callers permit.

### cadmpeg-codec-sldprt

#### SLDPRT3-01 · P2 · Remaining sketch quantizer aliases different coordinates through saturated i64 casts

**numerical; reproduced.** [cadmpeg-codec-sldprt/src/resolved_features/transforms.rs:626](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/transforms.rs#L626); [cadmpeg-codec-sldprt/src/resolved_features/relation_loci.rs:1571](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/relation_loci.rs#L1571); [cadmpeg-codec-sldprt/src/resolved_features/dimensions.rs:784](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/dimensions.rs#L784)

Points (1e14,0) and (2e14,0) at quantum 1e-6 produce the same (i64::MAX,0) key. Callers use these keys for equality, deduplication, lookup and integer sketch transforms, not merely candidate buckets with a geometric recheck. Earlier hole-key fixes used a separate local quantizer and did not repair this shared sketch helper.

Evidence: sld::saturated_keys; equality/deduplication and transform call inventory inspected.

Repair direction: Use a non-aliasing key representation or explicit out-of-range refusal. Audit integer transforms as part of the key change.

#### SLDPRT3-02 · P3 · Two line-distance copies accept perpendicular carriers as parallel after overflow

**numerical; reproduced.** [cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs:2231](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs#L2231); [cadmpeg-codec-sldprt/src/resolved_features/relation_loci.rs:1737](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/relation_loci.rs#L1737)

A horizontal carrier from (0,0) to (1e200,0) and a vertical carrier from (0,1) to (0,1e200) return a parallel-line distance of 1 instead of refusing nonparallel inputs. Both the cross product and the allowed product overflow to infinity, making the greater-than rejection false. Both wrappers retain the same arithmetic.

Evidence: sld::nonparallel_distance exercises the array helper with the production 1e-9 tolerance; the entity helper is source-equivalent.

Repair direction: Share a single normalized carrier-distance calculation and adapt SketchEntity extraction outside it.

#### SLDPRT3-03 · P3 · Ellipse membership accepts a remote point after an intermediate becomes NaN

**numerical; reproduced.** [cadmpeg-codec-sldprt/src/resolved_features/typed_relations.rs:947](../../../crates/cadmpeg-codec-sldprt/src/resolved_features/typed_relations.rs#L947)

An admitted ellipse centered at (-1e308,0), angle 0 and radii (1,0.5) accepts point (1e308,0). Subtracting the finite coordinates overflows; the rotation then multiplies infinity by zero and produces NaN. The rejection test abs(equation-1)>epsilon is false for NaN, and an unbounded ellipse returns true. A nonrepresentable intermediate must not certify membership.

Evidence: sld_ellipse::off_ellipse_accepted; the extracted full predicate uses the actual admitted SketchGeometry and a geometry-only SketchEntity scaffold.

Repair direction: Use finite-safe coordinate projection and require a finite equation before the membership comparison.

#### SLDPRT3-04 · P3 · Configuration state hashes copy filtering and ordering policy

**duplicate; source-reviewed.** [cadmpeg-codec-sldprt/src/history/hash.rs:83](../../../crates/cadmpeg-codec-sldprt/src/history/hash.rs#L83); [cadmpeg-codec-sldprt/src/history/hash.rs:96](../../../crates/cadmpeg-codec-sldprt/src/history/hash.rs#L96)

Both discard empty state maps, project configuration id plus one selected state map, sort by id and hash the same tuple-list representation. The selected payload differs. No present hash disagreement was demonstrated.

Evidence: Full bodies compared; hash_records remains the shared serializer.

Repair direction: Share projection/filter/sort framing through a private callback or iterator helper while preserving the exact serialized tuple shape and output hash.

### cadmpeg-codec-step

#### STEP3-01 · P2 · Small-scale shear passes both similarity-transform gates

**numerical; reproduced.** [cadmpeg-codec-step/src/geometry.rs:76](../../../crates/cadmpeg-codec-step/src/geometry.rs#L76); [cadmpeg-codec-step/src/geometry.rs:136](../../../crates/cadmpeg-codec-step/src/geometry.rs#L136)

Equal-length columns meeting at 60 degrees, scaled by 1e-10, pass both 2D and 3D similarity tests. The max(1) scale floor makes the orthogonality allowance too loose relative to the columns. This admits a shear for paths that emit a single-scale STEP transformation operator. A large basis curve can make the resulting geometric distortion material even when the transform coefficients are small.

Evidence: step::small_shear; curve/surface support and transformed-pcurve writer callers reviewed.

Repair direction: Test relative lengths and normalized column orthogonality. Retain the codec’s acceptance thresholds; do not copy a different codec’s tolerance.

#### STEP3-02 · P3 · Typed numeric unwrapping is implemented twice in the reader

**duplicate; source-reviewed.** [cadmpeg-codec-step/src/reader/geometry.rs:3563](../../../crates/cadmpeg-codec-step/src/reader/geometry.rs#L3563); [cadmpeg-codec-step/src/reader/pmi.rs:1936](../../../crates/cadmpeg-codec-step/src/reader/pmi.rs#L1936)

Both recursively peel Value::Typed and accept the same Integer/Real variants. ValueExt::number intentionally handles only an unwrapped scalar, so blindly replacing these with number() changes behavior.

Evidence: Exact recursive body comparison and ValueExt implementation reviewed.

Repair direction: Add one owning typed-number operation, update both callers directly, and remove the two local functions.

### cadmpeg-ir

#### IR3-01 · P2 · Remaining residual checks can certify a nonzero distance as zero

**numerical; reproduced.** [cadmpeg-ir/src/eval.rs:3915](../../../crates/cadmpeg-ir/src/eval.rs#L3915); [cadmpeg-ir/src/eval.rs:3393](../../../crates/cadmpeg-ir/src/eval.rs#L3393); [cadmpeg-ir/src/eval.rs:3557](../../../crates/cadmpeg-ir/src/eval.rs#L3557); [cadmpeg-ir/src/validate/geometry_consistency.rs:24](../../../crates/cadmpeg-ir/src/validate/geometry_consistency.rs#L24)

An admitted unit-X line accepts (0,1e-200,0) with zero tolerance and returns parameter 0. Its final sum of squared residuals underflows. The tolerant-intersection evaluation and inversion paths retain the same arithmetic. The validation helper also retains this duplicate, although its positive allowance floor means the tiny-distance example alone does not demonstrate a validation error. Large finite distances overflow in each copy. This is outside the NURBS-specific witness closure repaired as IR2-05.

Evidence: ir::line_false_witness; related copies and caller tolerance policies inspected.

Repair direction: Use the existing robust Point3 distance operation in the remaining residual gates. Preserve each caller’s allowance policy.

#### IR3-02 · P2 · Eight remaining unary sweep laws lose representable values or chain derivatives

**numerical; reproduced.** [cadmpeg-ir/src/eval.rs:4933](../../../crates/cadmpeg-ir/src/eval.rs#L4933)

ARCTAN, ARCOT, ARCSEC, ARCCSC and ARCCSCH at x=1e200 with incoming derivative 1e300 return zero instead of a derivative with magnitude about 1e-100. COTH(400) similarly returns zero. SECH(720) returns zero value and derivative; CSCH(720) refuses the result even though value and chained derivative are representable. The repaired TANH/ARCSINH/ARCCOSH/ARCOTH cases are excluded.

Evidence: ir::remaining_unary_laws (eight operators).

Repair direction: Use scaled reciprocal/exponential forms and apply the incoming derivative before an intermediate underflow loses it.

#### IR3-03 · P3 · Law-curve reference traversal is copied within one validator

**duplicate; source-reviewed.** [cadmpeg-ir/src/validate/carriers_parameterization.rs:19](../../../crates/cadmpeg-ir/src/validate/carriers_parameterization.rs#L19); [cadmpeg-ir/src/validate/carriers_parameterization.rs:620](../../../crates/cadmpeg-ir/src/validate/carriers_parameterization.rs#L620)

The nested reachability collector repeats the module-level Edge/Algebraic recursive walk with the same HashSet<&str> result. Both express the same reference traversal.

Evidence: Exact source comparison; same types and recursive cases.

Repair direction: Call collect_law_curves from reachability and remove the nested copy.

## Evidence

Twenty-two focused Rust probes reproduce the twenty-two numerical entries. They compile extracted current production functions against cached IR types and selected evaluator support; they do not rebuild a workspace crate. The assertions confirm the defects exist. A successful probe run is not a passing regression suite for repairs.

Saved evidence includes [output](evidence/run.log), [exit status](evidence/run.exit), [compile output](evidence/compile.log), [captured harness](evidence/probe.rs), [source manifest](evidence/manifest.json), and [reproducer](evidence/reproduce.py). Run the reproducer from the repository root after a compatible IR rlib exists. It prints its temporary evidence directory.

No fresh Cargo build, workspace check, or corpus batch was run. Extracted helper probes prove the stated arithmetic behavior; corpus frequency and complete format-file reproductions remain unverified. Source-reviewed related copies are identified as such. Most P3 numerical cases require extreme finite exponents. Zero ranked findings means this pass established none, not that the crate is proven free of defects.
