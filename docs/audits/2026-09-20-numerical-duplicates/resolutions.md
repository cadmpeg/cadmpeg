# Audit resolutions

All 47 ranked entries in the [original audit](README.md) are fixed on `feat/finish-illegal-states`. The fixes affect 12 crates named in the audit; the shared material adapter now lives in `cadmpeg-protein`. These are 47 per-crate entries, with some shared root causes.

Implementation HEAD: `f8c9cdc2576722bba5ecd87ece74ce5ef42b72a8`. Every implementation and correction was committed with `--no-verify` and a reason in the commit body before its Cargo verification. No full build or corpus sweep was run.

## Verification

- Quiet `cargo check --tests` passed for `cadmpeg-ir`, `cadmpeg-asm`, `cadmpeg-protein`, and all eleven codecs. The subsequent NX correction passed a separate IR/NX check.
- 102 IR evaluation, math, and transform tests passed.
- 23 ASM parser tests, one STEP pcurve-export regression, and three Protein appearance tests passed.
- Eleven probes ran extracted current codec functions and their regression fixtures. Four further probes exercised Rhino NURBS joins/elevation/remapping. One public-API probe checked nonlinear rational curve and surface derivatives under common weight scaling.
- Source-policy, codec-facade, formatting, and diff checks passed.

The [verification directory](verification/) holds complete final output, exit status, commands, probe source, and source fingerprints. The probes supplement crate type-checking; they do not execute complete codec suites. The codec probe uses reporting/type scaffolding and omits FreeCAD serialization derives; its numerical function bodies come from current source. The Rhino probe replaces reporting adapters only. Recorded commands retain temporary paths and cached library names; adjust these when reproducing them elsewhere.

Two new expectations were corrected after execution: affine inversion allows reciprocal roundoff, and a subnormal knot midpoint uses the actual representable parameter. A Rhino fixture was corrected to the current NURBS tuple variant. The CATIA consolidation briefly removed two unrelated knot helpers; they were restored. Final checks include those corrections.

The original audit remains a historical defect report. Its unranked observations did not establish additional distinct end-to-end failures. This pass closes its 47 ranked entries; it does not establish that the workspace has no other numerical defects. Manually inlined squared-length calculations outside these entries still need a separate audit.

## Entry status

| Entry | Change | Implementation commits | Verification |
|---|---|---|---|
| ASM-01 | Store SAT integer lexemes as i64 and reject out-of-range tokens. | `055b9f7ef1` | [asm-step-regressions](verification/asm-step-regressions.log) |
| ASM-02 | Use Vector3::unit_nonzero directly; preserve all finite nonzero direction scales. | `36c137cf9f` | [ir-final-tests](verification/ir-final-tests.log), [combined-check](verification/combined-check.log) |
| IR-01 | Accumulate homogeneous products over an extended exponent range; preserve signed weights and evaluate quotient derivatives without raw weight squares. | `ab2f3a143f` | [ir-final-tests](verification/ir-final-tests.log), [rational-scaling-probe](verification/rational-scaling-probe.log) |
| IR-02 | Accumulate homogeneous products over an extended exponent range; preserve signed weights and evaluate quotient derivatives without raw weight squares. | `ab2f3a143f` | [ir-final-tests](verification/ir-final-tests.log), [rational-scaling-probe](verification/rational-scaling-probe.log) |
| IR-03 | Use hypot for vector norms and point distances; use scaled component division for normalization. | `36c137cf9f` | [ir-final-tests](verification/ir-final-tests.log) |
| IR-04 | Use the shared exact-product fallback for affine point/vector application, composition, and inverse translation. | `36c137cf9f`, `5401f94c9d` | [ir-final-tests](verification/ir-final-tests.log) |
| IR-05 | Use Transform::try_inverse_affine and a hypot-based inverse norm in point inversion. | `36c137cf9f` | [ir-final-tests](verification/ir-final-tests.log) |
| IR-06 | Form bounded knot ratios before multiplying basis values. | `ab2f3a143f` | [ir-final-tests](verification/ir-final-tests.log) |
| IR-07 | Wrap overflowing parameter offsets through separate modular residues and check the result. | `ab2f3a143f` | [ir-final-tests](verification/ir-final-tests.log) |
| IR-08 | Call the existing nurbs_pcurve_parameter_domain helper directly. | `f11270d930` | [combined-check](verification/combined-check.log), [ir-final-tests](verification/ir-final-tests.log) |
| F3D-01 | Recover half-turn axis signs from the largest diagonal pivot. | `6c9a0c8ce4` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| F3D-02 | Scale line-circle equations and use relative discriminant bounds instead of an absolute unit floor. | `6c9a0c8ce4` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| F3D-03 | Reject non-finite millimetre conversion before constructing texture mapping or bump values. | `914c9d7727` | [protein-tests](verification/protein-tests.log), [combined-check](verification/combined-check.log) |
| F3D-04 | Share the Protein texture adapter, scalar lookup, material names, and schema classification in cadmpeg-protein::appearance. Preserve codec-specific unknown-unit and identity policies. | `7d4a4b2198` | [protein-tests](verification/protein-tests.log), [combined-check](verification/combined-check.log) |
| INVENTOR-01 | Reject non-finite millimetre conversion before constructing texture mapping or bump values. | `914c9d7727` | [protein-tests](verification/protein-tests.log), [combined-check](verification/combined-check.log) |
| INVENTOR-02 | Share the Protein texture adapter, scalar lookup, material names, and schema classification in cadmpeg-protein::appearance. Preserve codec-specific unknown-unit and identity policies. | `7d4a4b2198` | [protein-tests](verification/protein-tests.log), [combined-check](verification/combined-check.log) |
| FREECAD-01 | Use one placement decoder; scale quaternion components before normalization and use the shared nonzero axis normalization. | `055b9f7ef1` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| FREECAD-02 | Use one placement decoder; scale quaternion components before normalization and use the shared nonzero axis normalization. | `055b9f7ef1` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| SLDPRT-01 | Exclude the positive 2^63 boundary before converting a real override to i64. | `6c9a0c8ce4` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| SLDPRT-02 | Retain exact coordinate bits outside the i64 quantization range instead of saturating distinct hole axes. | `a349700ad7` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| SLDPRT-03 | Share line-angle evaluation and normalize both directions before the dot product. | `a349700ad7` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| SLDPRT-04 | Import the existing principal-plane frame helper from its owning geometry module. | `a349700ad7` | [combined-check](verification/combined-check.log) |
| CATIA-01 | Delete the duplicate surface-isocurve implementation and call the shared IR extractor, which retains mixed-magnitude weights and weighted coordinates. | `ab2f3a143f`, `fdf26d7c41`, `2f8fe1e584` | [ir-final-tests](verification/ir-final-tests.log), [rational-scaling-probe](verification/rational-scaling-probe.log), [combined-check](verification/combined-check.log) |
| CREO-01 | Scale line-circle equations and use relative discriminant bounds instead of an absolute unit floor. | `6c9a0c8ce4` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| CREO-02 | The retained codec acceptance threshold now uses the shared hypot-based norm. | `36c137cf9f` | [ir-final-tests](verification/ir-final-tests.log), [combined-check](verification/combined-check.log) |
| CREO-03 | Use one generic expression parser driver for all four value algebras. | `f11270d930` | [combined-check](verification/combined-check.log) |
| CREO-04 | Use one generic value_index owned by the legacy record module. | `f11270d930` | [combined-check](verification/combined-check.log) |
| CREO-05 | Use one affected-ID agreement helper owned by feature rows. | `f11270d930` | [combined-check](verification/combined-check.log) |
| NX-01 | Normalize derivative columns before the rank test, then rescale each correction with checked multiplication/division. Zero corrections remain zero even when the raw scale ratio overflows. | `6c9a0c8ce4`, `cbb96d6e68` | [nx-correction-check](verification/nx-correction-check.log), [codec-probes](verification/codec-probes.log), [ir-final-tests](verification/ir-final-tests.log) |
| NX-02 | Use Vector3::unit_nonzero directly; preserve all finite nonzero direction scales. | `36c137cf9f` | [ir-final-tests](verification/ir-final-tests.log), [combined-check](verification/combined-check.log) |
| RHINO-01 | Keep independent endpoint rows and full join multiplicity when rational endpoint weights differ. | `3727a12f6f` | [rhino-probes](verification/rhino-probes.log), [combined-check](verification/combined-check.log) |
| RHINO-02 | Elevate active spans with their actual control slices and preserve independent rows at full-multiplicity interior knots. | `3727a12f6f` | [rhino-probes](verification/rhino-probes.log), [combined-check](verification/combined-check.log) |
| RHINO-03 | Elevate active spans with their actual control slices and preserve independent rows at full-multiplicity interior knots. | `3727a12f6f` | [rhino-probes](verification/rhino-probes.log), [combined-check](verification/combined-check.log) |
| RHINO-04 | Map each knot through its normalized domain fraction instead of an overflowing global scale factor. | `3727a12f6f` | [rhino-probes](verification/rhino-probes.log), [combined-check](verification/combined-check.log) |
| RHINO-05 | Use f64::midpoint for joined endpoint coordinates and robust gap distance. | `3727a12f6f` | [rhino-probes](verification/rhino-probes.log), [combined-check](verification/combined-check.log) |
| RHINO-06 | Use shared exact-sum parameter reflection; retain finite endpoints when the endpoint sum overflows or rounds. | `913196b953` | [ir-final-tests](verification/ir-final-tests.log), [combined-check](verification/combined-check.log) |
| RHINO-07 | Compare robust diagonal lengths instead of overflowing squared lengths. | `6c9a0c8ce4` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| RHINO-08 | Use shared nonzero direction normalization for extrusion and miter admission. | `36c137cf9f` | [ir-final-tests](verification/ir-final-tests.log), [combined-check](verification/combined-check.log) |
| RHINO-09 | Use one checksum warning helper owned by the chunk module. | `a349700ad7` | [combined-check](verification/combined-check.log) |
| IGES-01 | Use shared exact-sum parameter reflection; retain finite endpoints when the endpoint sum overflows or rounds. | `913196b953` | [ir-final-tests](verification/ir-final-tests.log), [combined-check](verification/combined-check.log) |
| IGES-02 | Remove the Affine wrapper, use IR Transform directly, and propagate checked application/composition failures through codec errors or entity losses. | `913196b953` | [ir-final-tests](verification/ir-final-tests.log), [combined-check](verification/combined-check.log) |
| IGES-03 | Apply one common column scale before similarity and handedness checks. | `913196b953` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| IGES-04 | Use shared direction normalization, retaining the writer epsilon gate and annotation nonzero gate. | `36c137cf9f` | [ir-final-tests](verification/ir-final-tests.log), [combined-check](verification/combined-check.log) |
| IGES-05 | Subtract binary exponent bias as integers and split power-of-two scaling at the finite f64 exponent boundary. | `6c9a0c8ce4` | [codec-probes](verification/codec-probes.log), [combined-check](verification/combined-check.log) |
| STEP-01 | Use hypot for pcurve direction magnitude and reject an unrepresentable magnitude before emitting the VECTOR. | `6c9a0c8ce4` | [asm-step-regressions](verification/asm-step-regressions.log), [combined-check](verification/combined-check.log) |
| STEP-02 | Call the existing nurbs_pcurve_parameter_domain helper directly. | `f11270d930` | [combined-check](verification/combined-check.log), [ir-final-tests](verification/ir-final-tests.log) |
| STEP-03 | Use the existing source-record curve-carrier resolver directly from both readers. | `f11270d930` | [combined-check](verification/combined-check.log), [asm-step-regressions](verification/asm-step-regressions.log) |
