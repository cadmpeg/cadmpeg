<!-- SPDX-License-Identifier: Apache-2.0 -->
# Phase 2 performance gate and alloc/work/depth freeze record

Doc section 10 Phase 2 exit gate, items 5 and 8, plus the section 5.2
alloc/work/depth freeze. This record aggregates the six per-crate calibration
notes and the committed stage-1 harness baseline so the freeze rests on measured
charge profiles and the regression gate has an auditable outcome. The freeze
narrative below is a record, not a closure argument — the structural bounds it
cites are each defended in the owning crate's calibration note — but the
significant-regression gate it documents is live: the committed baseline records
per-entry measured wall time and peak allocation, and `baseline::perf_regressions`
fails the `cargo test` gate on an order-of-magnitude regression (see the
automated ratchet section).

## Freeze outcome

The envelope's `alloc_bytes` and `work` terms are frozen at `envelope-v2`
(`Envelope::PLATFORM_DEFAULT` in `crates/cadmpeg-ir/src/decode/policy.rs`).
Every envelope constant value is unchanged from `envelope-v1`: the Phase 2
calibration measured the migrated charge sites far inside the provisional
constants, so the data does not move them. The version bump records the status
transition (provisional starting point -> frozen, calibration-defended default),
giving every decode report a durable marker of which envelope freeze state
produced it.

Phase 2 also froze the `max_alloc_bytes`, `max_work`, and `max_depth` ceilings.
Those are ceilings in `ResourceLimits`, not envelope terms; `depth` in
particular has no envelope representation at all. Their values are likewise
unchanged, so the `desktop-v1`/`service-v1` profile tags do not advance —
profile tags track ceiling *values* so a value drift stays detectable
independent of the status freeze. The ceiling freeze is instead pinned
machine-readably by the `desktop_version_pins_its_ceilings` and
`service_version_pins_its_ceilings` tests, which assert each frozen ceiling
value against its tag.

Frozen dimensions and their calibration basis:

- `alloc_bytes`: base 64 MiB, k 64. Migrated charge sites are the container
  framing layers (all six codecs) plus the graduated count-framed leaves
  (rhino `cage`/`polyedge`/`hatch`, catia `value_block`/`catalog`/
  `object_graph`/`zero_entity`, f3d `container`/`act`). Measured cumulative
  `alloc_bytes` per fixture ranges from a few hundred bytes to low kilobytes,
  dominated by `grow_vec` accumulator nodes and retained-record copies, not by
  count-framed reservations (each proven by `View::counted` /
  `BoundedCount` before `exact_vec`).
- `work`: base 4,000,000, k 256. Migrated scans charge bytes examined, not one
  unit per iteration. Measured cumulative `work` per fixture is tens to low
  thousands of units (e.g. rhino cage = 38, polyedge = 15, hatch = 20; f3d act
  ~= 4x stream length over a ~0.3 KiB stream). Legacy leaf scans are single
  linear passes over already-charged decompressed streams, structurally bounded
  by stream length times a fixed pass count.
- `depth`: desktop gauge 256, service 64. No migrated module recurses beyond a
  handful of levels; rhino `cage`/`polyedge` hold `DepthGuard`s, and the codec-
  local caps (`MAX_CURVE_DEPTH`, `nurbs::MAX_KNOT_ENTRIES`) remain the load-
  bearing defense for the not-yet-threaded leaves. No fixture approaches the
  gauge.

Legacy leaf modules across all six codecs remain `legacy` in their
`parser-manifest.toml`; their alloc/work is bounded structurally (audited in
each calibration note) and observed process-wide by the harness peak-allocation
oracle, not yet by a budget charge. Per doc section 10 ("zero legacy modules is
a completion milestone, not a Phase-3 blocker") this does not block the freeze:
the freeze pins the constants against the charges that are real today, and the
constants are wide enough that threading the remaining leaves cannot move them.

## Performance gate results (against the pre-migration stage-1 baseline)

Source: `baselines/stage1.json`, 48 entries keyed
`codec x fixture x operation x profile` over all six codecs, both profiles, all
four operations. The stage-1 oracles are the section 7 performance surface.

| Gate dimension (doc section 10) | Oracle | Result |
| --- | --- | --- |
| fixture wall time | `wall_clock` | pass on all 48 entries (ceiling 10,000 ms) |
| peak RSS | `peak_alloc` | pass on all 48 entries (envelope 1,073,741,824 B) |
| semantic hash delta | `determinism` + `result_class` | no delta: `determinism` pass on all 48; every `result_class` reproduces its blessed value (30 `ok`, 10 `detect_high`, 6 `malformed`, 2 `detect_no`) |
| cumulative charged bytes | per-crate calibration notes | within the frozen envelope on every fixture (see freeze basis above) |

No oracle regressed: zero non-`pass` verdicts across the baseline. The migration
charging (cell updates, view construction, branch checks) did not push any
fixture past its wall-clock or peak-allocation ceiling, and no fixture changed
its decode semantics (identical IR JSON plus report digest across the decode-
twice determinism check, identical classified result). No significant
regression to flag under doc section 10; the outcome is recorded in
`docs/architecture.md`.

## Automated significant-regression ratchet

The pass/fail `wall_clock` and `peak_alloc` oracles fire only at the far
process-safety envelope (10 s, 1 GiB), so a safety refactor that makes a codec
an order of magnitude slower or hungrier while staying inside the envelope
passes every oracle. `baseline::perf_regressions` closes that gap: each baseline
entry records the *measured* wall-clock milliseconds and peak allocation, and the
regression gate fails when a current run exceeds its blessed measured value by
more than `PERF_REGRESSION_FACTOR` (4x), above absolute noise floors
(`PERF_WALL_FLOOR_MS` = 250 ms, `PERF_PEAK_FLOOR_BYTES` = 16 MiB). A regression
here requires an explicit re-bless — the section 10 "significant regressions
require explicit review" sign-off — to clear. An entry blessed before measured
values were recorded carries `0` and keeps a dormant ratchet until re-blessed, so
the oracle-verdict ratchet keeps gating in the meantime. The factor is loose by
design: it tolerates machine-to-machine variance and allocator noise while still
catching the order-of-magnitude regression the envelope oracles miss.

## Re-run

`cargo test -p cadmpeg-harness --test gate` runs the fast regression gate
against this baseline; `--ignored bless_baselines` re-blesses it after an
intended behavior change. The gate refuses to compare across a shifted
calibration, so the committed baseline's `envelope_version` tracks
`envelope-v3`.
