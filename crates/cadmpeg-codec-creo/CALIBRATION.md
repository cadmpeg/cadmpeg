# Creo decoder calibration note (Phase 2)

Per-module calibration observations for the doc section 5.2 alloc/work/depth
freeze. This note records what the migrated modules charge on the in-tree
fixtures and names the uncharged legacy sites that must be migrated before the
Creo contribution to the freeze is real. It is a report-only record, not a
closure argument.

## Migrated modules

All three graduated modules are context-independent primitive decoders over
caller-owned slices. None threads `DecodeContext`, so none charges any budget
dimension.

| Module | Peak alloc charged | Work charged | Depth | Basis |
|---|---|---|---|---|
| `psb.rs` | 0 | 0 | 0 | token/int/float reads are bounds-checked `Option` probes; `tokens` accumulation is proportional to input length, not an untrusted count |
| `scalar.rs` | 0 | 0 | 0 | `ScalarCache` grows one entry per distinct `0x46` token in `0..section.len()`; `decode` probes are bounds-checked |
| `datum.rs` | 0 | 0 | 0 | `planes` accumulates one candidate per byte position in `0..payload.len()`; `scalars` fills a fixed 10-wide row; all reads bounds-checked |

Observation: on the crate test fixtures (87 unit tests, whole-file decode in
`tests.rs`) the migrated modules perform no charged allocation and no work
charge. They contribute nothing to the alloc/work envelope and do not move the
freeze constants. Determinism holds: repeated decode of every fixture yields an
identical token stream and scalar cache.

## Legacy sites to charge before freeze

These are the count-framed and traversal sites whose allocation and work are
currently uncharged. Their contribution to the alloc/work profile cannot be
measured until they route through `read_counted`/`exact_vec` and a stated cost
model. Named here so the freeze step knows the Creo profile is incomplete.

Bound audit (source-verified): the Creo counts are not attacker-unbounded.
`psb::compact_int` is a two-byte varint whose value ceiling is 16383, and the
scalar-slot counts are products of two single-`u8` header fields (<=65025). So
every per-record allocation below is capped at a small constant (tens of KiB to
~1 MiB), and the number of records is bounded by input length because each
consumes bytes. The migration obligation is the ratchet — remove the disallowed
`with_capacity`/`resize` methods, charge `alloc_bytes`/`work`, and thread
`View`/`req_*` — not the suppression of an unbounded DoS.

- `feature.rs` — `with_capacity` + `0..count` fills at lines 871, 886, 2188,
  2497; declared-count loops at 980, 1099, 1252, 1383. Counts capped by two `u8`
  fields (871/2015) or the 16383 varint ceiling. Largest ratchet surface.
- `surface.rs` — `with_capacity(count)` + `resize(count, None)` at 612/622 sized
  by two `u8` fields (<=65025 slots); `compact_int`-count fill/collect at 397/404
  (<=16383).
- `container.rs` — `Vec::new`+push id collection at 702 (no disallowed method;
  capped by the 16383 varint and a no-progress break, needs only a `work`
  charge); summed namespace counts at 388 (no allocation);
  `with_capacity(hits.len())` at 332 bounded by the scan. One audited
  `View::window()` egress at 923 (section splitter input).
- `curve.rs` — superlinear `framed_segment` suffix search (O(section^2)-
  O(section^3)); no untrusted-count allocation, but the honest cost model is not
  proportional-to-input and is uncharged.
- `topology.rs` — edge graph at 201 (bounded by decoded rows, uncharged work).
- `decode.rs` — per-record orchestration, no per-record work charge.

## Freeze recommendation

Do not freeze the Creo alloc/work contribution yet. The migrated primitives
charge nothing by construction; the meaningful allocation and work live in the
count-framed structural readers above, all still legacy. Re-run this note after
those modules graduate, recording peak `alloc_bytes` and `work` per fixture,
before the section 5.2 alloc/work/depth freeze closes.
