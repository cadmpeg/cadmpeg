# Sixth-pass repairs

All 19 findings in the sixth-pass audit are fixed on `feat/finish-illegal-states`.

- `e2644ecf88`: the three duplication repairs.
- `e9aa479f73`: the sixteen numerical repairs and owner-scoped regression cases.
- `d83e39e178`: the diagonal-incidence correction found during post-commit verification.

Each implementation commit preceded compilation. Each used `--no-verify` with the user-requested reason in its commit body.

| Finding | Change |
|---|---|
| ASM6-01 | Normalize common weights before homogeneous circle recognition; compare degree-reduction residuals at each coordinate's scale. |
| CATIA6-01 | Project onto unit circle/ellipse axes and divide by radius once. |
| CREO6-01 | Compute cylinder–sphere heights with normalized radii. |
| CREO6-02 | Solve cone–sphere and cone–torus meridian intersections with the shared line–circle solver. |
| CREO6-03 | Compute torus section heights in a radial scale without an absolute unit floor. |
| CREO6-04 | Compute cylinder and plane–cylinder generator offsets in normalized coordinates. |
| F3D6-01 | Reflect with a unit normal and fused displacement arithmetic. |
| F3D6-02 | Use one body/face attribute selector with the existing color, name, persistent-link, timestamp precedence. |
| FREECAD6-01 | Use one iterative model/parameter curve census; count every trimmed/offset wrapper and leaf. |
| IGES6-01 | Evaluate parabola focal ratios with scaled products and quotients; use checked endpoint parameter division. |
| IR6-01 | Solve through line distance and half-chord length; accumulate the determinant from original coordinates to preserve exact diagonal incidence. |
| IR6-02 | Compare unit directions and compute finite normal projections. |
| IR6-03 | Project spans relative to their first endpoint with checked sums. |
| IR6-04 | Compare unit endpoint tangents and compute checked signed projections. |
| NX6-01 | Reflect line origins with a checked affine sum; form hyperbolic coefficients with scaled sinh/cosh products. |
| RHINO6-01 | Compute orientation in translated, scaled local coordinates with compensated accumulation. |
| RHINO6-02 | Measure legacy vertex gaps with chained hypot. |
| RHINO6-03 | Share comma-list property serialization through the wire owner. |
| SLDPRT6-01 | Compute unsigned line angles with atan2 of unit cross magnitude and dot product. |

## Verification

- `cargo check -q --tests` passed for all ten affected crates. The exact package selection is in [check-command.json](check-command.json); the complete output is [check.log](check.log), exit 0.
- Five focused IR tests passed with the schema feature after `e9aa479f73`; [command](ir-test.command.json), [output](ir-test.log), exit 0.
- The additional diagonal witness first failed, as saved in [regressions-before-diagonal-fix.log](regressions-before-diagonal-fix.log). Its assertion was retained and extended with exact binary scale cases in the production owner's tests.
- After `d83e39e178`, `cargo check -q -p cadmpeg-ir --tests` passed; [command](followup-check.command.json), [output](followup-check.log), exit 0.
- All 15 final standalone Rust regression tests passed; [output](regressions.log), [source and cached-library fingerprints](regression-sources.json). These tests cover all sixteen numerical findings; some tests cover several related findings.
- The repository source-policy check passed after the main numerical commit; [output](source-policy.json), exit 0. The follow-up changes only the determinant calculation and its named-constant regression.
- The three refactors were checked against the former selection order, wrapper accounting, serialization body, and complete caller inventories. The affected Rust test targets type-check.

[reproduce.py](reproduce.py) extracts current functions and owner regression cases. Run it from the repository root with `tree-sitter` and `tree-sitter-rust` available to Python and a built `cadmpeg-ir` rlib in `target/debug/deps`. It saves source fingerprints, full compiler/test output, and exit codes before displaying output. It omits the exact-sum module's own test-module declaration in the standalone copy and widens the extracted line–circle function's visibility for its Creo callers.

The standalone harness uses cached IR types and unchanged math helpers. Rhino orientation uses a minimal vertex/ring container; IGES focal distance and Rhino legacy tolerance use extracted expressions. These are numerical owner checks, not complete codec decode/export tests. The full workspace, codec runtime suites, corpus conversions and goldens were not run. No wire shape changed. Other agents' changes and the pre-existing untracked `rust_out` file were left untouched.
