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

### design.rs::decode_body_members (migrated site; module still legacy)

- `alloc_bytes`: the `BodiesRoot` member list reserves through `exact_vec` under
  `View::counted(count, 11)`, so the count is floored by the window (window
  bytes / 11) before the domain cap (100_000) even applies. On the committed
  `generated_design_bulkstream` fixture the count is 2, an 22-byte reservation;
  the previous unfloored `Vec::with_capacity(count)` could reserve ~4 MB against a
  hostile count over a short payload, which the physical-floor proof now
  forbids. The cross-entry accumulator grows through `grow_vec`.
- `work`: charged in two parts — the whole-window `BodiesRoot` prefix scan
  charges `win_len` once per Design bulk stream, and each member record charges
  a fixed 11 units (the record stride) before its reads. Linear in stream bytes,
  no super-linear term.
- `depth`: none.
- The rest of `design.rs` (the `parse_sketch_relation` member/return loops,
  domain-capped at 64, and the ~14 `&[u8]` reader functions) is still on the
  legacy path and is excluded from the freeze until the module graduates with
  the deny lint. This leg also hardened two pre-existing panics the new
  truncation test exposed (`decode_entity_headers` `bytes[start + 20]` and
  `decode_sketch_points` `payload[..112 + shift]`), converting both to checked
  `get`; these are correctness fixes, not budget changes.

## Legacy modules — pending leaf migration

`alloc_bytes` and `work` profiles for `sab`, `brep`, `nurbs`, `design`
(except the migrated `decode_body_members` site above),
`materials`, `history`, and `asm_header` are **not yet measurable**: the
count-framed loops, knot expansion, and recursion in these modules still run on
the legacy `&[u8]` read path and do not charge the budget. Their target charge
models are recorded in `parser-manifest.toml`. The alloc/work/depth freeze
(doc section 5.2 Phase 2 schedule) cannot be taken for f3d until these modules
graduate and their cumulative charge profiles are captured on the committed
fixtures. `nurbs.rs` (knot/pole counts, weight streams) is expected to dominate
the f3d `alloc_bytes` and `depth` profiles and must be calibrated last.

## Leg findings feeding the next migration pass

Investigation-only observations from this pass; no leaf graduated (the shared
worktree could not absorb a large `View`/`DecodeContext` re-plumb build-clean
this session). Recorded here so the freeze step and the next leg target the
right sites:

- The workspace `clippy.toml` already carries the `disallowed-methods` list
  (`Vec::with_capacity`/`reserve`/`reserve_exact`/`resize`/`resize_with`,
  `iter::repeat_n`), so the `#![deny(clippy::disallowed_methods)]` markers on
  the already-migrated modules are enforcing today. A leaf cannot carry the deny
  marker until every disallowed reservation in it is gone, so no partial deny
  marker can land ahead of the full reservation migration.
- `decode_body_members` in `design.rs` was the priority target and is now
  migrated (see the design.rs calibration entry above): the unfloored
  `Vec::with_capacity(count)` is replaced by `View::counted(count, 11)` +
  `exact_vec`, `grow_vec` on the accumulator, and per-record/per-scan `work`.
  The four `parse_sketch_relation` reservations (member and return loops,
  domain-capped at 64) are now converted to `Vec::new` + `push`: the record
  stride is variable (`marked_u32` + `next_reference_marker`), so no physical
  floor justifies `exact_vec`, and the cap already bounds the worst case to 64
  elements. After this conversion `design.rs` holds NO disallowed-method
  reservation and the deny lint would pass clean; it stays legacy only because
  the ~14 reader functions still read `scan.entry_bytes` `&[u8]` rather than a
  threaded `View`, and marking `migrated` with hostile reads on the raw slice
  would violate the checklist. Reservation-side work on `design.rs` is done;
  the residual is purely the `View` + `req_*` re-plumb of the readers. No alloc
  or work delta on fixtures: the loops run to the same 261+2 passing tests, and
  the 64-cap means peak reservation was already bounded; the change trades a
  one-shot capped reservation for amortized `push` growth (identical peak, at
  most a few reallocations up to 64 elements).
- `sab.rs` has NO stack recursion: the subtype `depth` in `frame` and
  `payload_subtype_range` is an iterative `i32`/`usize` counter over the token
  stream. Its migration is `View` + `work`-per-token + `grow_vec` on the
  `records`/`tokens`/`name_parts` accumulators, not DepthGuard. The manifest
  entry was corrected accordingly.
- Blocking chain for the low-risk leaves: `history.rs` cannot drop its raw-byte
  egress until `sab::frame` hands back `View`s instead of `&[u8]` offsets;
  `asm_header.rs` is held by encoder/`brep`/`container`/`decode` callers that
  pass `&[u8]`. Test coupling is the dominant blocker on `sab` (88 direct
  `tests.rs` call sites against `frame`/`payload_*`).
