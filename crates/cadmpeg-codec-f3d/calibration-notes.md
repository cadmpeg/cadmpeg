<!-- SPDX-License-Identifier: Apache-2.0 -->
# f3d Phase 2 calibration notes

Per-module calibration observations for the doc section 5.2 alloc/work/depth
freeze. Constants stay provisional until every high-risk leaf module has
graduated and these numbers are re-measured against the section 7 harness
peak-allocation telemetry. Numbers here are the charge *model* and the
container-layer observations available at this migration point; the leaf
`alloc_bytes`/`work` profiles are pending each module's leaf migration and are
marked so.

## Migrated modules

### container.rs (Phase 1 framing, graduated Phase 2)
- `alloc_bytes`: `PER_ENTRY_GRAPH_BYTES` charged per central-directory entry
  before any reservation; the entry accumulator is now `Vec::new` + `push`.
- `decompressed_bytes`: charged incrementally per `ExpandWriter::write` over
  each inflated entry; per-expand and cumulative envelope enforced.
- `work`: per-entry classification scan.
- Peak alloc on the committed fixture set is dominated by the largest inflated
  `.smbh` entry; the container layer holds no unbounded reservation. Concrete
  peak-RSS numbers must come from the harness subprocess telemetry, not the
  in-process tests, and are not re-measured in this pass (no leaf charge sites
  changed).

### act.rs (graduated Phase 2)
- `work`: charged once per whole-stream scan pass, proportional to the readable
  window length (`win_len`). Three passes per ACT bulk stream —
  `decode_root_components` and `decode_channel_groups` charge `win_len` each,
  `decode_table` charges `2 × win_len` (ACTTable marker search plus the trailing
  GUID scan). Total ≈ `4 × win_len` per matching `FusionACTSegmentType` bulk
  stream. On the committed `generated_act_bulkstream` fixture (≈0.3 KiB) this is
  a few hundred work units; the model is linear in stream bytes with a fixed
  factor of four, no super-linear term.
- `alloc_bytes`: the `ACTTable` record list reserves through `exact_vec` under a
  `BoundedCount` (`min_element_size = 15` bytes/record, domain cap
  `MAX_TABLE_ENTRIES = 100_000`), so the worst-case table reservation is bounded
  by both the physical floor (window length / 15) and the cap. Accumulators
  (`entities`, `guids`, `root_components`, per-table GUIDs, channel groups) grow
  through `grow_vec`, charging one element before each reservation. The `by_key`
  merge map charges `ACT_ENTITY_GRAPH_BYTES = 128` per merged node against the
  input-proportional allowance. On the fixture the peak `alloc_bytes` is a few
  KiB, dominated by the merged-entity graph nodes, not the table reservation.
- `depth`: none — ACT has no recursion.
- Provisional constants touched: none load-bearing yet; `MAX_TABLE_ENTRIES`,
  the 1024-unit UTF-16 cap, the 128-byte ASCII cap, and the 1..=8 channel cap
  are format facts retained as defense in depth. The section 5.2 alloc/work
  freeze still waits on the count-framed topology leaves (`brep`, `nurbs`),
  which dominate the profile; act's charges are captured here as a first
  migrated-leaf datapoint.

### decode.rs, records.rs, native.rs, history_records.rs
- No budget charge of their own (orchestration / pure record model). Nothing to
  calibrate; excluded from the freeze inputs.

## Legacy modules — pending leaf migration

`alloc_bytes` and `work` profiles for `sab`, `brep`, `nurbs`, `design`,
`materials`, `history`, and `asm_header` are **not yet measurable**: the
count-framed loops, knot expansion, and recursion in these modules still run on
the legacy `&[u8]` read path and do not charge the budget. Their target charge
models are recorded in `parser-manifest.toml`. The alloc/work/depth freeze
(doc section 5.2 Phase 2 schedule) cannot be taken for f3d until these modules
graduate and their cumulative charge profiles are captured on the committed
fixtures. `nurbs.rs` (knot/pole counts, weight streams) is expected to dominate
the f3d `alloc_bytes` and `depth` profiles and must be calibrated last.
