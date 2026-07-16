<!-- SPDX-License-Identifier: Apache-2.0 -->
# sldprt Phase-2 calibration note

Per-module calibration observations for the doc section 5.2 alloc/work/depth
freeze. This note records what the sldprt decode path charges on fixtures today
so the freeze step has a measurement basis. Constants are **not** frozen from
this note; alloc/work/depth freeze only after every sldprt leaf module emits
real charges (doc section 5.2: freeze "per dimension, after the phase that makes
its charges real").

## What charges today

Only `container.rs` drives budget counters, from the Phase-1A/1B container
context:

- `charge_work` over the outer marker scan, proportional to source-image bytes
  examined by the scanner (`scan_view`).
- `decompressed_bytes` charged incrementally by `begin_expand`/`ExpandWriter`
  as each block's DEFLATE output streams in `EXPAND_CHUNK`-sized writes, before
  the arena grows (`admit_block`). Block inflation is already migrated off raw
  `read_to_end`.
- `charge_alloc` per admitted space-graph record (one for each decompressed
  block `Transform` space, one per Parasolid `Slice`/`Transform` stream) and per
  retained owned buffer (source image, inflated payloads).

No leaf module (`brep/*`, `history`, `metadata`, `pmi`, `tessellation`,
`parasolid`, `resolved_features`, `decode`) charges yet: they parse over `&[u8]`
windows with the legacy `cadmpeg_ir::read`/`le` readers and are listed `legacy`
in `parser-manifest.toml`. Their calibration rows are therefore pending, not
measured.

## Observations (report-only, desktop-v1 profile)

Measured qualitatively from the in-crate `tests.rs` fixtures. These are
starting-point observations for the freeze, not proposed constants.

| Module | Peak alloc basis | Work charged | Depth |
| --- | --- | --- | --- |
| `container.rs` | source image + Σ inflated block payloads; charged as `alloc_bytes` cumulatively, so peak-live is lower than the counter | `work` ≈ bytes scanned for markers across the source image | flat (no recursion) |
| leaf geometry/history/metadata modules | untrusted-count `Vec::with_capacity` and marker-scan `Vec::new`/`push`, currently outside the budget | none (pending View migration) | `brep/graph.rs` recurses without a `DepthGuard`; depth basis unmeasured until the guard lands |

## High-risk sites still uncharged (freeze blockers)

These are the count/decompression/recursion sites that must charge before the
sldprt alloc/work/depth freeze is meaningful (each is a `legacy` row with the
same residual in `parser-manifest.toml`):

- `brep/spline.rs`: `Vec::with_capacity(descriptor.control_count)`,
  `Vec::with_capacity(u_count * v_count)`, `iter::repeat_n(value, multiplicity)`
  — untrusted-count allocation, unbudgeted.
- `brep/topology.rs`, `brep/entity.rs`, `pmi.rs`: `Vec::with_capacity(count)`
  from untrusted counts.
- `parasolid.rs` (`inflate_zlib_prefix`) and `resolved_features.rs` (the
  writer/patch match path, `flate2::read::ZlibDecoder` + `read_to_end`): raw
  inflation with no pre-charge ceiling (class-A decompression sink); the
  migrated path charges `decompressed_bytes` at `ExpandWriter::write`.
  (`container.rs` block inflation is already routed through `begin_expand` and
  is not in this list.)
- `brep/graph.rs`: entity-graph recursion without a `DepthGuard`.

## Freeze recommendation

Do not freeze sldprt alloc/work/depth from this batch. `decompressed_bytes` is
already real at the container (block inflation streams through `begin_expand`),
but alloc/work/depth are not yet charged by any leaf. Re-run this note after the
container-to-leaf `View` plumbing lands (`ContainerScan` retaining the
derived-space `View`/`SpaceId`, leaves reading through it, and the raw
`parasolid.rs`/`resolved_features.rs` inflation routed through `begin_expand`)
so the leaf rows carry real cumulative charge profiles.
