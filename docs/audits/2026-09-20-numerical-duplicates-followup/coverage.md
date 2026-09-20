# Coverage and candidate disposition

The census parsed Rust function and method bodies in all 21 crate directories. It excluded conventional test-only modules/directories and test functions. It compared exact token bodies and bodies with consistently renamed identifiers, then screened numerical operations, conversions, and likely geometry helpers. Generated or macro-expanded functions were not expanded. Counts describe screening coverage, not individually proven functions. Selected numerical and duplicate candidates received source review and the focused probes described in the findings.

The highest-impact demonstrated issues were ranked first within each owning crate. Related call sites sharing one defect family are grouped; inherited defects are not counted again in every dependent crate. No finding was dropped to meet the 30-per-crate cap.

## Areas reviewed

| Crate | Candidate review in this pass |
|---|---|
| `cadmpeg-ir` | NURBS knot extraction, rational bounds and witnesses, surface inversion, periodic parameter mapping, law differentiation, helix admission, and repeated curve/surface operations. Eight numerical families are ranked. |
| `cadmpeg-codec-iges` | Homogeneous knot insertion/decomposition, ruled-rail alignment, closure certification, and remaining norm candidates. Two numerical families are ranked; the copied IR decomposition is part of the first. |
| `cadmpeg-codec-f3d` | Both intersection paths, profile speed bounds, endpoint grid bucketing, image-scope decoding, and remaining distance candidates. Endpoint buckets use saturating neighbors and a geometric recheck, so a saturated cell is not by itself proof of an incorrect merge. |
| `cadmpeg-codec-freecad` | Placement versus topology-transfer arithmetic, similarity admission, triangulation normals, GUI appearance/list operations, and B-rep table readers. The earlier quaternion repair was not reported again. |
| `cadmpeg-codec-creo` | Quadratic solvers and their callers, trim intersections, local norm/frame checks, legacy ownership lookup, and scalar-array storage. The earlier line/circle repair was not reported again. |
| `cadmpeg-codec-nx` | JT transform admission, intersection tangent checks, remaining offset/blend numerical candidates, and OM registry/row duplication. The earlier least-squares repair was not reported again. |
| `cadmpeg-codec-rhino` | Legacy B-rep reconstruction arithmetic, localizer child readers, mesh diagonal selection, and remaining curve/surface candidates. The corrected NURBS elevation/join algorithms were excluded from new findings. |
| `cadmpeg-codec-sldprt` | Tessellation error calculation, B-rep projection ownership, unit-vector record admission, and remaining distance/angle candidates. Visibility changes by another worker were not treated as audit fixes. |
| `cadmpeg-asm` | Procedural ellipse/support-cone lengths, shared normalization, text/binary geometry carrier paths, and fit-patch wrappers. The shared direction normalization repair does not cover the ranked radius calculations. |
| `cadmpeg-codec-step` | Tessellation mass properties and validation-property callers, pcurve normalization/transform helpers, and typed reader/writer candidates. The pcurve magnitude fix from the earlier audit was not repeated. |
| `cadmpeg-codec-catia` | Unit-direction admission/normalization, endpoint bucket matching, angle and arc helpers, and candidate duplicate bodies. Different record-specific unit tolerances were not treated as duplicate bugs. Geometric rechecks prevent treating bucket saturation alone as a false weld. No additional ranked defect was established. |
| `cadmpeg-codec-inventor` | Remaining line-carrier comparisons, bounded native counts, external-reference/OLE value readers, and native adapter candidates. No additional ranked defect was established. |
| `cadmpeg-codec-sat` | Detection and codec forwarding. Geometry is owned by ASM; ASM2-01 is counted there, not repeated here. |
| `cadmpeg-core` | Numeric byte assembly, allocated lengths, view conversion bounds, budget arithmetic, and distinct-key visitors. Reviewed integer narrowing is bounded by the containing type/read contract. BTreeMap and HashMap visitors deliberately retain different collection semantics. No additional ranked defect was established. |
| `cadmpeg-container` | Archive/compound size and sector arithmetic candidates, chain limits, and checked ranges. This was not a complete malformed-container security audit. No additional ranked numerical/duplicate defect was established. |
| `cadmpeg-protein` | Page framing, terminal used-byte lengths, and checked logical offsets. Shared adapter fixes from the prior audit were not reported again. |
| `cadmpeg-parasolid` | Schema-token length/offset arithmetic and classification helpers. No numerical geometry implementation or additional ranked duplicate was found. |
| `cadmpeg-registry` | Registry/catalog function-body candidates. No numerical geometry implementation or additional ranked duplicate was found. |
| `cadmpeg` | Scalar inspect window/width conversion, checked range selection, and query/inspection candidates. No additional ranked numerical/duplicate defect was established. |
| `cadmpeg-test-support` | Function-body duplicate and numeric candidate screening of shared harness utilities. No additional ranked defect was established. |
| `cadmpeg-fuzz` | Function-body screening of target/seed helpers. Synthetic constant-sized payload construction was not treated as production parsed-count arithmetic. No additional ranked defect was established. |

## Similarity observations not ranked as defects

- IR `RevolveConstruction::{extent,extent_mut}`, `{axis,axis_mut}`, and `{profile,profile_mut}` have matching bodies but different borrow contracts. A macro or extra abstraction would not remove an independently maintained algorithm.
- `NurbsPoles3::apply_points` and `PcurveNurbsPoles::apply_points` have matching iteration shape over different dimensional types. The invariant and callback types differ; no new numerical defect was demonstrated there.
- Creo `DimensionedScalars::fill_tokens` and `CountedScalars::fill_tokens` contain the same short length-check/unzip/assignment body. This is a real small duplicate. The dimensioned versus counted storage contracts differ; consolidating eight lines alone does not justify adding a trait. It is recorded here rather than ranked as a separate actionable layer defect.
- NX fixed-lane/wire record conversions and IR analytic constructors share syntax but carry distinct format or domain contracts. Their similarity is not a reason to combine those contracts.
- ASM procedural curve/surface fit patchers each locate a different carrier and already delegate the scalar write. Their shared framing shape is not equivalent to a duplicate geometry algorithm.
- Several remaining manual squared norms validate a source vector already required to be unit length. Rejecting a huge vector there is correct. Several distance checks reject distant points; overflow to infinity alone does not prove a wrong admission result.

## Verification boundaries

The 25 probes exercise the ranked numerical families with small constructed inputs. Multiple probes can support one finding, and some related call sites are established by source inspection. A fixture that confirms a bug is not a passing regression test for a repair. No source fix is included.

The extracted harness substitutes minimal law/container scaffolding, not different arithmetic. It links cached IR/core types and selected evaluator support. Its successful compilation does not type-check the production crates. Source SHA-256 values, a captured harness, full command/output, and exit statuses are retained beside the report. No fresh workspace build or full-file corpus run was performed.

Some numerical cases require extreme finite exponents. Their mathematical failures are established, but their occurrence in user CAD files is not measured. The helper-level similarity/shear and spline-boundary witnesses likewise do not establish a corpus frequency. Further format-specific fixtures would quantify that reach; they are not prerequisites for the demonstrated arithmetic counterexamples.
