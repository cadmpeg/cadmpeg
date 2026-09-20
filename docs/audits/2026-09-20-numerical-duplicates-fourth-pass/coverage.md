# Coverage

| Crate | Production functions screened | Ranked findings | Review result |
|---|---:|---:|---|
| `cadmpeg` | 390 | 0 | Inspect numeric decoding, range arithmetic and query counts screened. Struct offset loops are protected by a checked total span; small hex format copies add no numerical policy. |
| `cadmpeg-asm` | 608 | 1 | Transform patch unit conversion reproduced. Shared normalization and rational cache scanner repairs remain in place. |
| `cadmpeg-codec-catia` | 2023 | 2 | Analytic membership and section-carrier agreement reproduced. Unit-direction admission intentionally rejects nonunit source vectors; that is not a normalization bug. |
| `cadmpeg-codec-creo` | 2326 | 2 | Profile crossings reproduced and section collectors reviewed. Unit-frame checks, conic candidates, normalized knot vectors and existing quadratic owners screened. |
| `cadmpeg-codec-f3d` | 3211 | 4 | Spatial dimensions, line/conic membership and mesh transforms reproduced. Body/face tag-choice functions have parallel dispatch, but a new abstraction is not justified by this pair alone. |
| `cadmpeg-codec-freecad` | 877 | 1 | Implicit extrusion magnitude reproduced. Periodic-knot extension, placement and topology range normalization screened. Parallel 2D/3D census functions operate on different enums. |
| `cadmpeg-codec-iges` | 1104 | 4 | Local evaluators, conic decoding/writing, interval geometry, trimming and similarity operations reviewed. The parabola evaluator discrepancy is dormant under the current NURBS-only pcurve factory. |
| `cadmpeg-codec-inventor` | 539 | 0 | Carrier matching and shared appearance adapter screened. The repaired independent direction normalization remains. No additional concrete failure or useful algorithm consolidation established. |
| `cadmpeg-codec-nx` | 3039 | 2 | Damped intersection solve reproduced. Periodic lifting, closest-parameter candidates, wire rows and JT operations screened. Normalized source unit-vector checks are not arbitrary-vector normalization routines. |
| `cadmpeg-codec-rhino` | 1056 | 1 | Trim breakpoint reversal reproduced. Plane mapping, knot periodicity, miter formulas, mesh casts and legacy distance expressions screened. The writer tolerance floor rules out a claimed tiny-gap underflow admission at that floor. |
| `cadmpeg-codec-sat` | 31 | 0 | Host layer delegates kernel geometry to cadmpeg-asm. Detection, dialect and decode functions screened; no separate numerical implementation found. |
| `cadmpeg-codec-sldprt` | 2321 | 3 | Radius keys and helix fitting reproduced; remaining distance wrappers reviewed. Source unit-direction gates and geometry payload guards screened. |
| `cadmpeg-codec-step` | 791 | 0 | Unit resolution, periodic edge ranges, transforms and lexical/numeric projection screened. Suspicious raw periodic differences were not ranked without a reachable violating caller input. |
| `cadmpeg-container` | 92 | 0 | Archive ranges, compound-file counts, sector ownership and compression limits screened. Checked span/allocation paths reviewed; no new demonstrated numerical failure. |
| `cadmpeg-core` | 303 | 0 | View arithmetic, allocation/budget limits, integer assembly and distinct map visitors screened. Collection-specific visitors do not warrant a generic layer solely to remove repeated syntax. |
| `cadmpeg-fuzz` | 203 | 0 | Seed generation casts and bounded mutation arithmetic screened. Seed lengths are constructed locally; no new production numerical defect established. |
| `cadmpeg-ir` | 2906 | 10 | Transforms, point/differential evaluation, sketch validation, angular pcurves, planar intersections and polyline interpolation reviewed and reproduced. Mutable/immutable accessors and dimension-specific pole types were not ranked as duplicate algorithms. |
| `cadmpeg-parasolid` | 23 | 0 | Detection, prologue length and dialect functions screened. No numerical geometry evaluator lives in this crate. |
| `cadmpeg-protein` | 37 | 0 | Property framing and shared distance conversion screened. Existing finite checks in the repaired appearance adapter remain. |
| `cadmpeg-registry` | 67 | 0 | Format/catalog, dialect and source-selection functions screened. Repeated registry plumbing adds no numerical algorithm to consolidate. |
| `cadmpeg-test-support` | 58 | 0 | Golden comparison, key normalization and dialect test support screened. No additional concrete numerical or duplicate-policy finding. |

The AST census excludes test-only modules and test trees. Screening covers the workspace; it is not an exhaustive proof that every function is correct. Numerical and cast patterns identify candidates, followed by selected source and caller review. No crate reached the 30-item cap. Similar failures in one owner are grouped by their common repair.
