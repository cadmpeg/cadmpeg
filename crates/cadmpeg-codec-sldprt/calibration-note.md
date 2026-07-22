<!-- SPDX-License-Identifier: Apache-2.0 -->

# cadmpeg-codec-sldprt calibration note (Phase 2)

Per-module allocation and work observations for the doc section 5.2 freeze step.
This note records the current (pre-graduation) state so the freeze has a
baseline; the bindable numbers below cannot be finalized until the modules
actually charge work, which is blocked on the missing platform API (see
`parser-manifest.toml` `[meta]` and the leg shared_requests).

## Branch reality

The migrated-means checklist depends on `cadmpeg-ir` primitives that are absent
on this branch: `View`/`req_*`/`View::window`/`View::counted`, `BoundedCount`,
`grow_vec`, `alloc_unfloored`, `DepthGuard`, and `begin_expand`/`ExpandWriter`.
The only bounded primitives available are `cadmpeg_ir::cursor::{Cursor,
bounded_len, Cursor::counted, Cursor::read_counted}`. Consequently no sldprt
decoder module currently charges work, and peak allocation is governed by the
existing hand-rolled ceilings rather than by an `alloc_unfloored` cap or a
`begin_expand` decompressed-bytes charge. The freeze cannot bind a work ceiling
until those charges exist.

## Decompression ceilings observed today

| Site                              | File:line                                                           | Ceiling today                                                                                                                                         | Post-graduation target                                                     |
| --------------------------------- | ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Per-block raw-DEFLATE inflate     | `container.rs:337`                                                  | `Vec::with_capacity(uncomp.min(1<<20))`, then `inflated.len()==uncomp` and CRC-32 verified                                                            | `begin_expand` charging `decompressed_bytes` per chunk before arena growth |
| Parasolid member inflate          | `parasolid.rs:55` -> `cadmpeg_ir::compression::inflate_zlib_prefix` | uncapped `ZlibDecoder::read_to_end`                                                                                                                   | bounded inflate through `begin_expand`                                     |
| Resolved-feature match re-inflate | `resolved_features.rs:~18859`                                       | HARDENED this leg: `Take(target.len()+1)` bounds inflation, so a member expanding past the target length cannot match and is never fully materialized | `begin_expand` charging `decompressed_bytes`                               |

The remaining uncapped site is `parasolid.rs:55` -> `inflate_zlib_prefix`, which
lives in `cadmpeg-ir` and cannot be capped from within this crate's scope; it is
the crate's residual decompression-bomb exposure. `container.rs:337` is bounded
by a declared-size + CRC-verified `1<<20` pre-reserve, and the resolved-feature
re-inflate is now `Take`-bounded using present primitives; both awaiting only the
`begin_expand`/`ExpandWriter` decompressed-bytes charge for the freeze.

## Count-framed allocation sites and their current floors

| Site                                                                                   | File:line                             | Floor today                                                                                                             | Amplification risk                    |
| -------------------------------------------------------------------------------------- | ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| topology refs                                                                          | `brep/topology.rs:48,56`              | family constants (4/5/6)                                                                                                | none (constant)                       |
| entity refs                                                                            | `brep/entity.rs:75`                   | `slot_count` <= 9                                                                                                       | none (constant)                       |
| spline curve poles                                                                     | `brep/spline.rs:324`                  | guarded `control.len()==control_count*dimension`                                                                        | low                                   |
| spline surface poles                                                                   | `brep/spline.rs:483`                  | `u_count*v_count == poles` via `infer_surface_shape` `checked_mul==Some(poles)` gate; `poles = control.len()/dimension` | none (control-derived, overflow-safe) |
| pmi values                                                                             | `pmi.rs:485`                          | `bounded_len` + hand-rolled 1024 cap                                                                                    | low                                   |
| resolved-feature id/entry/address arrays                                               | `resolved_features.rs:3640,3683,5772` | `bounded_len`                                                                                                           | low                                   |
| resolved-feature components/links                                                      | `resolved_features.rs:3906,4036`      | adjacent `.filter(                                                                                                      | count                                 | (1..=64))`/`(1..=2)`before`with_capacity(count)` | none (bounded <=64 / <=2) |
| parasolid points                                                                       | `parasolid.rs:161`                    | `scalar_count/3` bounded by body length                                                                                 | low                                   |
| tessellation / metadata / annotations / appearance / classification / history / native | (module-wide)                         | none (no cursor adoption)                                                                                               | UNMEASURED                            |

## Peak allocation (fixtures)

Not yet measured under an instrumented allocator. Charging is absent, so a
meaningful peak-alloc / work-charged table per fixture cannot be produced this
leg. Every count-framed `with_capacity` site above is already floored with
present primitives (adjacent range filters, `bounded_len`, or control-derived
`checked_mul` products); the residual gap to migrated is work-charging and View
threading, not a missing floor. Once `begin_expand` and `grow_vec` land, re-run
against the `corpus/` and `seeds/` sldprt
fixtures and record: peak resident bytes, total `decompressed_bytes` charged,
and total count-framed `reserve` bytes, per fixture. The freeze binds sldprt's
alloc/work/depth ceilings from those numbers.

## Recursion / depth

`brep/graph.rs` reference resolution is flat iteration over pre-collected
vectors (`graph.rs:78-152`), not stack recursion; but the coedge ring /
next-pointer chain walk can cycle on hostile input. A `DepthGuard` (or visited
bound) plus node-plus-edge work charging is the freeze target for depth. No
depth ceiling is bindable until that guard exists.
