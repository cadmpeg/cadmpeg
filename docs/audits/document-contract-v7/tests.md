# Document contract verification

This is a **historical, pre-addendum snapshot**. It records the exact executable and assertions exercised before the D/P/W/B/E corrections. It does not establish conformance of the current wire, native dispositions, canonical dimensions, recompute state, or revised E1–E5 tests. Fresh validation of those changes is still required.

The integrated document-contract suite passed **10 tests, zero failures**, in **24.18 seconds** on 2026-10-19. A separate source-less public-fixture collection passed all **26 admitted Rhino fixtures and 12 FreeCAD fixtures**. The fixture collection records actual reductions and does not equate a successful write with losslessness; see [coverage artifacts](coverage.md).

## Executable and source identity

The tested executable SHA-256 was `b3393f5438709cb3c2cd265e139edfd5b137ff030dfe7ea6d0fc26c8ccd9a1f5`.

Source and public-input manifests were captured before compilation and after the run. The Rust/TOML/lock/source-JSON fingerprint remained `c6616a6683d72c75dd759e242de3a20f6648da7f15ab2b8393384ac2ca10f8bb`; no recorded source or fixture changed during the build and run. The executable hash also remained unchanged throughout the subsequent 38-fixture collection. These identifiers describe the executed snapshot rather than promising that later source changes were tested automatically.

## Integrated E1, E2, E3 checks

Command:

```sh
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo test -q -p cadmpeg --test document_contract -- --test-threads=1
```

| Test | Result | Assertions exercised |
|---|---|---|
| E1: edited CADIR with stale sidecar, then sidecar deleted | Passed | Stale hash binding refuses; deletion admits source-less Synthesized output; feature name, parameter expression, dimensional value/expression, and bound color survive; scope/fidelity, counts, and declared Written content agree. |
| E1: library editor rebinds historical sidecar | Passed | Historical decode losses do not reject the CADIR parse; edited inherited output synthesizes with `PatchDeclined`; all four edits survive; synthesis reason appears once; Info/Error export floors follow actual losses. |
| E1: unedited Patched output versus source-less Synthesized output | Passed | Both public design fixtures agree under one global identity bijection on all nonempty declared Written arenas. |
| E2: all admitted Rhino goldens | Passed | All 26 positive fixtures synthesize and re-decode with report/document scope agreement. Two malformed `reject_*` fixtures are excluded from this positive-output check. |
| E2: CLI neutral body-name and appearance edits | Passed | Edited names and bound colors override retained attributes, with explicit native omission coverage and losses. |
| E2: translated library edits with rebound history | Passed | Edited coordinates and geometry/topology counts survive; retained source cannot restore old coordinates. |
| E2: V8 declared-Written comparison | Passed conditionally | The selected comparison follows actual coverage. Captured Rhino reports currently declare no neutral arenas Written, so this assertion selects no neutral arenas; the separate name/color/translation/count assertions carry the substantive E2 checks. |
| E3: default partial-design export and current-decode loss policy | Passed | Unreadable/vendor records report losses; default output remains readable; decode refusal and Blocking floor judge the actual native decode; independent geometry survives. |
| E3: source guard and unreadable-object synthesis | Passed | Unrepresented design cannot reach patching; synthesis omits the bad object, preserves independent geometry, and does not restore old source records. |
| E3: authored replacement and geometry-only state | Passed | Explicit native and neutral history deletion prevents resurrection; one authored Box remains exactly one feature; geometry-only output has no construction features and preserves bodies. |

No golden files were regenerated for these integration runs. Earlier failed snapshots exposed real problems: native product properties overwrote current feature/sketch edits, GUI output with no camera incorrectly declared absent records unrepresented, and omitted objects left dangling dependencies. Writer corrections made the checks pass; scope, fidelity, and exact Written-content assertions were retained.

The exact identity helper still requires a single global bijection preserving counts, scalar values within repository tolerances, nested ordering, and reference aliasing. Candidate-pruning optimizations reduced runtime without turning numeric near-equality into an equivalence relation or allowing per-arena identity matches to disagree.

## Direct CLI policy reproductions

The public Rhino `generated_point.3dm` produces only Info-severity decode losses. A direct four-case check with the executable above observed:

| Severity floor | `--reject-lossy=decode` result |
|---|---|
| Info | Refused, `decode_loss_rejected` |
| Warning | Passed |
| Error | Passed |
| Blocking | Passed |

A CATIA A8 parameter-map input was freshly authored from the codec's existing public synthetic grammar. It is not a copy of the absent historical frozen fixture, and no PR #207 or private bytes were used. It delivered two entities, full scope, transferred geometry, and one Blocking topology loss. Native conversion with `--reject-lossy=decode` refused. After dump to CADIR, the same conversion passed with the adjacent historical sidecar and after its removal; the history heading appeared only with the sidecar, and model content/scope agreed.

Both reproductions have focused CLI regression tests. The manually executed CLI cases above are distinct from the complete compiled CLI test-harness gate.

## Remaining release gates

This document records the executed checks above. Repository-wide source/schema/format/test gates, cross-codec Full/Geometry golden conformance, the one-shot wire-v7 artifact regeneration, external FreeCAD application acceptance, and private-corpus checks have their own evidence. They must be completed and recorded before publication; this E1/E2/E3 result does not substitute for them. Private files, names, and paths are excluded from the public audit.
