# Third audit resolutions

All 33 ranked entries are repaired: 22 numerical defect families and 11 consolidation candidates. Changes remain on `feat/finish-illegal-states`. No wire schema or golden snapshot changed.

Every implementation and correction was committed with `--no-verify` before compilation. Commit bodies state the user-requested reason.

## Changes

| Finding | Resolution | Initial implementation |
|---|---|---|
| ASM3-01 | Unique cache scanning shares width, marker and ambiguity handling; curve and surface grammars stay separate. | `cdc4e33fc6` |
| CATIA3-01 | Surface refinement calls the shared column-scaled least-squares solver. | `c13aa55bf4` |
| CATIA3-02 | Rigid chart fitting scales centered coordinates before covariance; review also covers translated charts with small local spread. | `c13aa55bf4` |
| CREO3-01 | Line/circle intersection uses scaled roots and checks the returned point against the circle. | `409e437769` |
| CREO3-02 | Circle tangency uses relative geometry and verifies both radial residuals. | `409e437769` |
| CREO3-03 | Line intersection normalizes each direction before the angular rank check. | `409e437769` |
| CREO3-04 | Profile line/arc and arc/arc predicates use the shared scaled planar solvers. | `409e437769` |
| CREO3-05 | Cross-section selection, relocation and sorting share one collector without a new record trait or extra result allocation. | `cdc4e33fc6` |
| CREO3-06 | Replay ID agreement uses one iterator consensus function. | `cdc4e33fc6` |
| F3D3-01 | Segment intersection compares orientation signs; uncertain signs use exact products. | `409e437769` |
| F3D3-02 | Both arc intersection paths share circle geometry; coincident-arc overlap retains its own policy. | `409e437769` |
| F3D3-03 | Point distance uses hypot; segment projection scales direction and displacement independently. | `409e437769` |
| F3D3-04 | Point and text decoding share design-stream selection and metadata association. | `cdc4e33fc6` |
| F3D3-05 | Timestamp and optional generated ordinal share an entry and one target dispatch; source precedence and model-order numbering are preserved. | `cdc4e33fc6` |
| IGES3-01 | BRep and CSG checks call Point3::distance; the local squared-distance copy is removed. | `13dfe81fd6` |
| INVENTOR3-01 | Sketch line agreement normalizes vectors before direction and displacement products. | `c13aa55bf4` |
| NX3-01 | Null-vector cofactors use row scaling and hypot; the chord fallback normalizes its tangent without raw squares. | `c13aa55bf4` |
| NX3-02 | Periodic lifting uses a finite remainder fallback and one implementation for offset and blend callers. | `c13aa55bf4` |
| NX3-03 | Intersection enrichment and chart sampling use Point3::distance. | `13dfe81fd6` |
| NX3-04 | Labeled and unlabeled operation visitors share section-link resolution and source offsets. | `cdc4e33fc6` |
| NX3-05 | Pattern and trim-surface predicates share absence-of-body-evidence logic. | `cdc4e33fc6` |
| RHINO3-01 | Plane parameter mapping preserves endpoints and uses a scaled ratio and interpolation. | `c13aa55bf4` |
| RHINO3-02 | Periodic knot gaps and their tolerance are compared in a scaled frame. | `c13aa55bf4` |
| RHINO3-03 | Legacy curve and surface objects use one RhinoIO envelope reader. | `cdc4e33fc6` |
| SLDPRT3-01 | Grid keys preserve coordinates outside i64 range; integer transforms explicitly refuse them. Hole and sketch keys share the grid owner. | `48660eb47d` |
| SLDPRT3-02 | Parallel line distance compares unit directions, and the entity form delegates to the array geometry owner. | `48660eb47d` |
| SLDPRT3-03 | Ellipse membership rejects nonfinite displacement and residual calculations. | `48660eb47d` |
| SLDPRT3-04 | Configuration state hashes share keyed-record ordering and hashing with unchanged serialized inputs. | `cdc4e33fc6` |
| STEP3-01 | Similarity checks compare relative lengths and normalized dot products in 2D and 3D. | `c13aa55bf4` |
| STEP3-02 | Geometry and PMI readers use ValueExt::typed_number; plain-number parsing keeps its original admission policy. | `cdc4e33fc6` |
| IR3-01 | Curve inversion, support separation and geometric validation use the shared hypot-based point distance. | `13dfe81fd6` |
| IR3-02 | Eight inverse and reciprocal hyperbolic laws retain extended-range chain derivatives and stable values. Review also covers subnormal ARCCSCH arguments. | `13dfe81fd6` |
| IR3-03 | Carrier reachability uses the existing recursive law-curve collector. | `13dfe81fd6` |

[Machine-readable resolutions](resolutions.json) identify the production owners and test selections. Commit `dcf60f1f4e` contains the review corrections described above.

## Verification

The final scoped check selects all 11 changed packages with `cargo check -q` and `--tests`. The focused test run selects the numerical regressions and affected parser, feature and relation owners. No `cargo build`, workspace-wide test run, corpus conversion or golden regeneration was run.

All final checks and selected tests passed. The first regression batch passed 602 tests; the final regression batch passed 1,339. The additional owner batch passed 296. The batches overlap and are not a count of distinct tests.

| Package | Final regression batch | Additional owner batch |
|---|---:|---:|
| ASM | 106 | — |
| CATIA | 25 | — |
| Creo | 71 | 40 |
| F3D | 88 | 34 |
| IGES | 7 | 22 |
| Inventor | 1 | — |
| NX | 234 | 33 |
| Rhino | 31 | — |
| SolidWorks | 734 | — |
| STEP | 11 | 158 |
| IR | 31 | 9 |

Source-policy and codec-facade results are recorded with the checks. Quiet logs with exit status 0 are successful checks. [Commands, complete output and exit statuses](evidence/verification/commands.json) include the initial failures and their corrections. [Source fingerprints](evidence/verification/source-manifest.json) identify this task’s repaired files.

The first check found one NX child-module call to the removed distance helper and a private-parent import in the new IR fixture. A later check found an incorrect transform import in the new STEP fixture. Commits `19325de738` and `ba41afc5ed` corrected those references before recompilation. No existing test expectation was changed.

## Scope and limits

The regressions establish the audited triggers and the selected owner contracts. They do not establish correctness for every CAD file or every floating-point input. SolidWorks coordinate keys preserve distinct out-of-range points, while integer-grid transforms refuse such points. Nonfinite or unrepresentable geometric results remain refused.

Other workers committed unrelated CLI refusal tests and IGES FEM test placement during this session. Those edits were not staged or committed by this task. Uncommitted Rhino writer test-placement edits also appeared at the end of verification and were left untouched. Checks ran in the shared checkout.

The original findings and defect-confirming probe remain historical evidence. That probe asserts the old failures; use the recorded Cargo commands to verify the repaired tree.
