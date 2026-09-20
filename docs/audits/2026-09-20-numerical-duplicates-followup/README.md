# Follow-up numerical and duplicate-function audit

Date: 2026-09-20. Branch: `feat/finish-illegal-states`. This is a new audit after the [previous 47-item repair](../2026-09-20-numerical-duplicates/resolutions.md). Production source was not changed by this audit.

**30 ranked new findings: 21 numerical defects and 9 duplicate/forwarding implementations.** All 21 crate directories were included in the function census and candidate screening (20 workspace members plus the excluded fuzz crate). No crate has more than 8 ranked findings; the requested 30-per-crate cap did not remove any ranked item.

The highest-priority defects are wrong IR/IGES Bezier spans and false IGES boundary equality. Further ordinary-coordinate failures include the small Fusion line/circle example, scaled Creo quadratics, FreeCAD similarity admission, and translated STEP mass properties. Extreme-exponent findings are identified explicitly in the details.

## Per-crate results

| Crate | Function bodies screened | Numerical | Duplicate/layer | Total |
|---|---:|---:|---:|---:|
| `cadmpeg` | 390 | 0 | 0 | 0 |
| `cadmpeg-asm` | 610 | 1 | 0 | 1 |
| `cadmpeg-codec-catia` | 2,017 | 0 | 0 | 0 |
| `cadmpeg-codec-creo` | 2,334 | 2 | 1 | 3 |
| `cadmpeg-codec-f3d` | 3,212 | 2 | 1 | 3 |
| `cadmpeg-codec-freecad` | 883 | 2 | 3 | 5 |
| `cadmpeg-codec-iges` | 1,121 | 2 | 0 | 2 |
| `cadmpeg-codec-inventor` | 539 | 0 | 0 | 0 |
| `cadmpeg-codec-nx` | 3,040 | 1 | 2 | 3 |
| `cadmpeg-codec-rhino` | 1,056 | 1 | 1 | 2 |
| `cadmpeg-codec-sat` | 31 | 0 | 0 | 0 |
| `cadmpeg-codec-sldprt` | 2,322 | 1 | 1 | 2 |
| `cadmpeg-codec-step` | 792 | 1 | 0 | 1 |
| `cadmpeg-container` | 97 | 0 | 0 | 0 |
| `cadmpeg-core` | 306 | 0 | 0 | 0 |
| `cadmpeg-fuzz` | 203 | 0 | 0 | 0 |
| `cadmpeg-ir` | 2,958 | 8 | 0 | 8 |
| `cadmpeg-parasolid` | 23 | 0 | 0 | 0 |
| `cadmpeg-protein` | 37 | 0 | 0 | 0 |
| `cadmpeg-registry` | 67 | 0 | 0 | 0 |
| `cadmpeg-test-support` | 58 | 0 | 0 | 0 |

Zero means no additional confirmed ranked finding in this pass. It does not prove the crate has no defect. SAT and other codecs can inherit shared ASM/IR failures; those are counted once at their implementation owner. CATIA has no additional ranked finding beyond the preceding repair in this pass.

## Evidence and limits

- [Ranked findings](ranked-findings.md): exact owners, triggers, consequences, and repair direction, ordered within each crate.
- [Machine-readable findings](findings.json) and [coverage notes](coverage.md).
- [Reproducer](evidence/reproduce.py) extracts current source into a small Rust harness. [Captured Rust](evidence/probe.rs), [source fingerprints](evidence/source-manifest.json), and [complete output](evidence/probe.log) record the audited state.
- 25 focused probes reproduced the reported failures; compiler and probe exit statuses were 0. The assertions deliberately confirm the demonstrated defects; passing does **not** mean the project is fixed. The IR cached-public-API smoke probe duplicates the independently extracted current boundary-witness test.
- No Cargo build, workspace test run, or corpus conversion was performed. The harness uses existing cached IR/core libraries for types and selected evaluator support. Law and mesh container scaffolding is minimal; the law arithmetic and mesh measurement functions are copied from current source unchanged. The Rhino mean, helix admission, and IR witness probes extract the exact relevant expressions from their larger owners.
- Duplicate findings were reviewed in source; syntactic similarity alone was not treated as a behavior defect. Refactor benefit is qualified where the output types differ.
- Concurrent visibility edits were present. Fingerprints and final checks distinguish source changes from numerical evidence. No other worker's changes were staged or reverted.

Run from the repository root: `python3 docs/audits/2026-09-20-numerical-duplicates-followup/evidence/reproduce.py`. It writes artifacts to a temporary directory and requires `rustc` plus compatible cached `cadmpeg_ir`/`cadmpeg_core` rlibs. It does not run Cargo.
