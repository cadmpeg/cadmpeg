# Fourth audit repairs

All 30 findings in the [audit](../README.md) are fixed: 27 numerical defects and three duplication groups across ten crates. The original audit and its failing-behavior probes remain unchanged. [resolutions.json](resolutions.json) maps every finding to its repair commit and regression coverage.

The repairs were committed with `--no-verify` before compilation, as requested. The commit bodies record that reason. Follow-up fixes were committed before their verification runs. All changes remain on `feat/finish-illegal-states`.

## Verification

- `cargo check -q --tests` passed across all ten affected crates. IGES passed another scoped check after the constant-token compatibility correction.
- Focused regressions passed for ASM, Creo, F3D, FreeCAD, NX, Rhino and SolidWorks. The first nine-crate run passed 40 tests and exposed two invalid new fixtures. The affine fixture now uses an admitted NURBS carrier; the IGES fixture now declares double-precision tokens. The numerical assertions were retained.
- The corrected IR/IGES run passed 90 tests. The subsequent CATIA/IGES run passed 72 tests. The final IGES writer/conic run passed 101 tests. These counts overlap; they are not a count of distinct tests.
- The scoped Clippy pass found style issues in IR, IGES and NX. The follow-up commit preserves behavior, and `cargo clippy -q --tests --no-deps` passed for those three crates after correction.
- Source policy and the codec facade check passed.

Complete output, commands and exit statuses are in [verification.json](verification.json) and [logs](logs/). Early compiler failures include two missing imports, then an in-progress CATIA test move by another worker. The imports were corrected in a follow-up commit; the other worker completed the test move. Both were resolved before the successful ten-crate check.

No full workspace test suite, corpus batch, schema generation or golden regeneration was run. No IR/native record shape changed. The final lint-only edit was checked without repeating runtime suites whose numerical behavior it preserves.

## Repairs

| Finding | Change |
| --- | --- |
| ASM4-01 | Range-safe native translation conversion; reject nonfinite results before writing. |
| CATIA4-01 | Measure transverse vectors directly and use length residuals. |
| CATIA4-02 | Compare scaled squared lengths with finite guards; retain the existing squared-residual tolerance. |
| CREO4-01 | Use exact orientation signs for crossings and perpendicular distance for endpoint tolerance. |
| CREO4-02 | Use one section-record collector over selected iterators. Preserve the Xsections name and marker predicate. |
| F3D4-01 | Use Point3::distance and reject nonfinite measured values. |
| F3D4-02 | Compare perpendicular distance in length units and retain segment parameter bounds. |
| F3D4-03 | Require finite operands in analytic membership comparisons. |
| F3D4-04 | Use shared inverse-transpose normal transformation and exact determinant orientation. |
| FREECAD4-01 | Use Vector3::norm for the implicit extrusion length. |
| IGES4-01 | Delete the private evaluator and call the shared IR pcurve evaluator. |
| IGES4-02 | Delete private basis and weighted loops; call the repaired shared IR evaluators. |
| IGES4-03 | Classify coefficient signs without multiplying them; scale zero tests relative to the coefficients and preserve finite radius ratios. |
| IGES4-04 | Rescale conic coefficients by a common binary factor; refuse unrepresentable sets before record emission. Center the coefficient exponent range to retain precision; preserve the ordinary -1 token. |
| NX4-01 | Normalize the matrix and right-hand side before column norms and normal products; compare scaled residual norms. |
| NX4-02 | Remove the offset distance wrapper and update all callers to Point3::distance. |
| RHINO4-01 | Reflect each reversed interior knot with the shared range-safe parameter helper. |
| SLDPRT4-01 | Use GridCoordinate for radius keys at both producers and consumers; preserve finite identities outside i64 range. |
| SLDPRT4-02 | Fit circle coefficients in normalized coordinates and restore model units after solving. |
| SLDPRT4-03 | Remove graph and sketch-writer distance wrappers and call Point3::distance. |
| IR4-10 | Scale parabola vertex and chart-axis vectors while keeping focal distance and parameterization unchanged. |
| IR4-01 | Use Point3::distance and reject nonfinite measurements. |
| IR4-02 | Delete raw affine point/vector helpers and propagate checked Transform results through all callers. |
| IR4-03 | Compute the determinant sign with the shared exact-product accumulator. |
| IR4-04 | Use Homogeneous accumulation and quotient differentiation for 2D NURBS; retain a finite point when a derivative is unavailable. |
| IR4-05 | Share atan2 differentiation with extended-range products for harmonic and rational polar curves. |
| IR4-06 | Normalize great-circle slope coordinates and differentiate the angular map before applying the azimuth chain rule. |
| IR4-07 | Keep chain-rule factors inside extended-range products and quotients; avoid reciprocal and exponential intermediate range loss. |
| IR4-08 | Use exact difference quotients and checked convex interpolation for points and tangents. |
| IR4-09 | Scale direction and geometry independently; scale position differences before overflow and use checked intersection placement. |

The review also corrected two boundary cases within the repaired owners: sphere-section axis normalization for subnormal center offsets, and IGES coefficient precision near `1e308`. Ordinary IGES conic output keeps its existing constant token.
