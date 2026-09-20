# Follow-up audit resolutions

All 30 ranked findings are fixed: 21 numerical defects and 9 duplicate or forwarding implementations. Changes remain on `feat/finish-illegal-states`. No wire schema or golden snapshot changed.

Each implementation and verification correction was committed with `--no-verify` before compilation, as requested. Each commit body records that reason. The unchanged least-squares solver and its existing test were moved to IR in a separate first commit.

## Changes

| Finding | Resolution | Implementation commit |
|---|---|---|
| IR2-01 | Shared Bezier extraction inserts active endpoints and indexes each span from its actual knots, including full-multiplicity breaks. | `e1388405e3` |
| IR2-02 | Surface projection uses the shared scaled least-squares solver and unsquared residual norms. | `e1388405e3` |
| IR2-03 | Curve and pcurve search share a speed bound that removes common weight scaling before products. | `e1388405e3` |
| IR2-04 | Periodic curve mapping delegates to the finite periodic arithmetic and preserves its half-open canonical result. | `e1388405e3` |
| IR2-05 | Curve search retains unsquared Euclidean distances, including nonzero subnormal-scale residuals. | `e1388405e3` |
| IR2-06 | Quotient-law derivatives use extended-range product sums for numerator and denominator. | `e1388405e3` |
| IR2-07 | TANH, ARCSINH, ARCCOSH and ARCOTH use stable value and chain-derivative formulas. | `e1388405e3` |
| IR2-08 | Helix circular-frame admission uses the shared hypot-based vector norm. | `e1388405e3` |
| IGES2-01 | IGES now uses the corrected IR Bezier extraction, so boundary checks include the correct complete control polygons. | `e1388405e3` |
| IGES2-02 | Boundary cross-products and thresholds are compared in extended range; underflow cannot turn a nonzero difference into equality. | `e1388405e3` |
| F3D2-01 | Boolean line/arc intersection uses the point-producing owner; the duplicate quadratic implementation is removed. | `b3291d46c7` |
| F3D2-02 | Profile speed bounds delegate to the shared scale-safe IR bound. | `e1388405e3` |
| F3D2-03 | Canvas and decal decoding share stream/scope traversal and ID ordering in decode/image.rs. | `57c3cc27a2` |
| FREECAD2-01 | Similarity admission compares relative column lengths and dot products of unit directions. | `b3291d46c7` |
| FREECAD2-02 | Placed normals use unit_nonzero and refuse zero vectors instead of returning unnormalized vectors. | `b3291d46c7` |
| FREECAD2-03 | All four matching TechDraw list grammars share envelope, count and record-type validation; record parsers stay separate. | `57c3cc27a2` |
| FREECAD2-04 | Line and point styles share appearance and binding construction with explicit primitive style variants. | `57c3cc27a2` |
| FREECAD2-05 | Curve2ds, Curves and Surfaces share table framing, allocation bounds and trailing-token checks. | `57c3cc27a2` |
| CREO2-01 | Analytic and sketch equations use one normalized, cancellation-resistant real-root solver; sketch admission remains at its owner. | `b3291d46c7` |
| CREO2-02 | Circle intersection normalizes geometry before the tangent test and checks both radial residuals. | `b3291d46c7` |
| CREO2-03 | Curve and surface namespace lookup share one array-name-parameterized ownership check. | `57c3cc27a2` |
| NX2-01 | JT f32 transform rows are measured in f64, preserving finite extreme scales without loosening the orthogonality threshold. | `b3291d46c7` |
| NX2-02 | Index, linked and target rows share section framing, source-location resolution and admitted row numbering. | `57c3cc27a2` |
| NX2-03 | Class and field registries share deduplication and framed-section precedence; native record types remain distinct. | `57c3cc27a2` |
| RHINO2-01 | Legacy vertex accumulation scales coordinates before summing and restores scale after division. | `b3291d46c7` |
| RHINO2-02 | Optional localizer curve and surface values share anonymous-child framing and version/presence checks. | `57c3cc27a2` |
| SLDPRT2-01 | Arc subdivision uses an asin half-chord formula and reports sagitta through sine squared, including at the segment cap. | `b3291d46c7` |
| SLDPRT2-02 | Removed the forwarding projection helper; all callers import the IR projection owner directly. | `b3291d46c7` |
| ASM2-01 | Ellipse and both support-cone readers compute radii with hypot. | `b3291d46c7` |
| STEP2-01 | Mesh moments use a local reference point and a local extent for the volume significance test. | `b3291d46c7` |

[Machine-readable resolutions](resolutions.json) name each source owner and its regression or existing test selection.

## Verification

All scoped checks used `cargo check -q` with `--tests`. The checked packages were IR, ASM, Creo, F3D, FreeCAD, IGES, NX, Rhino, SolidWorks and STEP. No `cargo build`, workspace test suite, corpus conversion or golden regeneration was run.

| Suite | Tests passed |
|---|---:|
| IR full library | 894 |
| ASM selected owners and regressions | 22 |
| Creo selected owners and regressions | 39 |
| F3D selected owners and regressions | 43 |
| FreeCAD selected owners and regressions | 66 |
| IGES selected owners and regressions | 57 |
| NX selected owners and regressions | 72 |
| Rhino selected owners and regressions | 40 |
| SolidWorks selected owners and regressions | 20 |
| STEP selected owners and regressions | 7 |
| Total distinct selected tests | 1,260 |

The first codec test run passed 365 of 366 tests. The new STEP test failed during setup because its tessellation ID lacked the required identity syntax. Commit `45d5884095` corrected the fixture ID without changing its volume or centroid expectations. All six STEP validation tests then passed, including the translated tetrahedron case. The seventh STEP test had passed in the first run. The SolidWorks sagitta test also passed after its tolerance was named.

The first checks found unused imports in IGES and F3D; both were removed in follow-up commits. Source policy found formatted error constructors in the new FreeCAD table reader and absolute source links in the audit report; both were corrected. The repeated source-policy check and the codec-facade check passed.

[Complete output, commands and exit statuses](evidence/verification/commands.json) include the initial failures and the successful corrections. Quiet check logs with exit status 0 are successful checks. [Source fingerprints](evidence/verification/source-manifest.json) identify the repaired files.

## Scope and limits

The regression cases establish the audited numerical failures and the shared helper contracts. Existing owner tests cover the affected parsers and transfer paths. They do not establish correctness for every CAD file or every floating-point input. Unrepresentable rational weight ratios or coordinate products are refused where a conservative bound cannot be retained.

A concurrent visibility edit in `crates/cadmpeg-asm/src/brep/records.rs` was present during final verification. It was not edited, staged or committed by this task. Verification ran in the shared worktree; the source manifest covers this task's files.

The original findings, source fingerprints and defect-confirming probes remain historical evidence. The old reproducer expects the audit source revision and compatible cached libraries; it is not the verification command for the repaired tree. Use the recorded Cargo checks and test selections for the repaired code.
