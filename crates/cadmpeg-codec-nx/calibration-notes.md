<!-- SPDX-License-Identifier: Apache-2.0 -->
# NX Phase 2 calibration notes

Per-module observations for the doc section 5.2 alloc/work/depth constant
freeze. NX leaf scanners run over the inflated Parasolid stream, a
decompression `Transform` space whose every byte is already charged as
`decompressed_bytes` at `begin_expand` time (`parasolid.rs`). The freeze step
consumes the process-wide peak-allocation and cumulative-work telemetry the
section 7 subprocess harness records over this crate's fixtures and fuzz
targets; the figures below are the structural bounds each migrated module
guarantees, which the empirical telemetry must stay within.

## Charge model

- `decompressed_bytes`: charged in `parasolid::inflate_stream` per
  `ExpandWriter::write`, 8 KiB chunks, bounded by the per-expand and cumulative
  ceilings. This is the only production of new address-space bytes in the crate.
- `work`: charged once in `parasolid::extract_streams` as the part-payload
  length for the linear zlib-header scan (doc section 10 Phase 1A file-wide
  search charging). The leaf scanners do not hold a threaded context and add no
  further work charge; their scans are each a single linear pass over the
  already-charged inflated stream, so cumulative work is proportional to
  stream length times the fixed number of record-family passes, not to any
  untrusted count.
- `alloc_bytes`: not charged by the leaf modules (no threaded context). Each
  migrated accumulator is bounded structurally (below) and observed empirically
  by the harness allocation oracle.
- `depth`: no recursion in any NX parser module; `Graph::parse` and its
  extractors resolve references through a flat index, so no `DepthGuard` and no
  depth charge.

## Per-module allocation bounds (peak-alloc calibration basis)

| Module | Migrated accumulator | Structural peak bound |
|---|---|---|
| `om.rs` | offsets, records | `count <= bytes.len()/4` (index-array subtraction floor), extra `2..=100_000` cap |
| `nurbs.rs` | control points | `poles == payload.values.len()/stride`, bounded by materialized payload |
| `nurbs.rs` | knot vector | codec-local `MAX_KNOT_ENTRIES = 2^20` over the untrusted multiplicity sum |
| `intersection.rs` | parameters | materialized chart-point count |
| `topology.rs` | XMT sequences | `bytes.len()/2` regardless of requested count |
| `geometry.rs` | record carriers | count of valid records in the stream |
| `deltas.rs` | records/points | count of status-byte-framed records |

## Codec-local caps proposed for the freeze

- `nurbs::MAX_KNOT_ENTRIES = 1 << 20` — zero-floor knot expansion cap
  (multiplicities have no physical input floor). Defense in depth; the section 5
  budget is the load-bearing bound once a context is threaded here.
- `om::indexed_sections` count window `2..=100_000` — algorithm-limit cap over
  the entity-index count, redundant with the `bytes.len()/4` physical floor.
- `parasolid::MIN_INFLATED = 64` — coincidence-rejection guess, not a resource
  bound; the decompression ceilings bound inflation.

## Fixture coverage feeding the freeze

Truncation and value behavior for every migrated module is exercised by the
`cadmpeg-codec-nx` test suite (69 tests, all green) and the fuzz targets
`nx_om`, `nx_nurbs_curves`, `nx_nurbs_surfaces`, `nx_intersection`,
`nx_topology`, `nx_geometry_{points,curves,surfaces}`, `nx_deltas`, and
`nx_parasolid`. The harness runs these under desktop/salvage defaults; the
recorded peak RSS and cumulative charged bytes are the inputs to the
alloc/work freeze. No live peak/work figures are asserted here because the
budget counters are not exposed on the public decode report; they are read from
the subprocess harness telemetry at freeze time.
