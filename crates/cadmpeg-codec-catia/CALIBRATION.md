<!-- SPDX-License-Identifier: Apache-2.0 -->
# CATIA codec Phase 2 calibration notes

Per-module calibration observations for the doc section 5.2 alloc/work/depth
freeze. Values are cumulative charge profiles (section 5.3), not peak live
memory; the harness peak-RSS oracle uses its own, separately calibrated
constant. The `value_block`, `catalog`, and `variant` modules are migrated and emit
their charges today, as do `object_graph`, `native`, and `zero_entity`; for the
remaining leaf parsers the work and `alloc_bytes`
columns are the *intended* charge points, not yet emitted charges — they are
recorded here so the freeze step measures the migrated sites rather than
calibrating noise (section 5.2: "freezing a dimension nobody charges yet would
calibrate noise").

## Measurement basis

- Fixtures: the crate's `tests.rs` golden `.CATPart` inputs plus the
  `crates/cadmpeg-fuzz/fuzz_targets/catia_*` corpora reaching each committed
  path.
- `input_basis`: root file bytes + finalized reconstructed B-rep (`active_brep`
  Concat) length. The Concat assembly is the only site currently charged
  (`begin_derived_space`, `alloc_bytes`).

## Per-module observations

| module | charged today | peak cumulative alloc (fixture) | intended work charge | notes |
| --- | --- | --- | --- | --- |
| `container` | `alloc_bytes` (Concat), `decompressed_bytes` | reconstructed B-rep length | directory-scan bytes examined | only module emitting charges; Concat copy ~= sum of MainDataStream + SurfacicReps extents |
| `value_block` | `work`, `retained_bytes` (migrated) | payload copy bounded by `declared_len` | root-image bytes examined once by the `7C0B` probe | migrated: scan bytes charged as work up front; each payload charged through `charge_retained` before `to_vec()` |
| `catalog` | `work`, `alloc_bytes` (migrated) | entry table reserved exactly via `exact_vec` | root-image bytes examined once by the `7C02` probe | migrated: entry count proven by `View::counted` `BoundedCount(_, 1)`; capacity reserved exactly, no longer `with_capacity` |
| `object_graph` | `work`, `alloc_bytes`, `retained_bytes` (migrated) | record/head/field/list `grow_vec` accumulators + blob/context copies | root-image bytes per marker scan + each candidate's framed `total_len` | migrated: marker scans charged up front; candidate extents charged before the nested `7C09` walk; accumulators `grow_vec`; `0xe5` blob + `7CD9` context via `charge_retained`; pure `Option` probe so no `BoundedCount`/`req_*` |
| `zero_entity` | `work`, `alloc_bytes`, `retained_bytes` (migrated) | record `grow_vec` + retained record copies | whole-window bytes examined once by the `a9 03` walk | migrated: scan charged as work up front; zero-floor record stream via `grow_vec`; each record body via `charge_retained`; inner reference lanes bounded by a single count byte over the charged copy |
| `topology` | none (legacy) | 8 accumulators + hash indexes | spine + edge-row scan bytes, binding traversal nodes+edges | largest index footprint on standard-nested fixtures |
| `b5` | none (legacy) | 6 accumulators + object-id map | `b5 03` walk + object-id resolution | traversal cost dominated by object-id map size |
| `e5` | none (legacy) | 8 accumulators + ref indexes | class-record bytes + resolution nodes+edges | orientation tape via `repeat_n` -> `grow_vec` |
| `geometry` | none (legacy) | knot/mult/pole vectors | carrier-candidate scan bytes | 14 `with_capacity` sites; knot `repeat_n` expansions |
| `b5_transfer` | none (legacy) | IR entities + knot expansion | B5Graph traversal nodes+edges | needs `DepthGuard` on face->loop->coedge descent |
| `native` | none direct (migrated) | 0 direct; delegates to the three migrated leaves | 0 direct; leaf-charged | migrated: `decode` performs no hostile read; one-to-one typed transforms of already-charged records, sized by `records.len()` |
| `decode` | `work` (entity-decode unit) | binding accumulators | per-record commit at variant dispatch | charges one entity-decode work unit today |
| `variant` | none (pure) | 0 | 0 | migrated: no read, no scan, no allocation |

## Freeze guidance

- `alloc_bytes`, `work`, `depth` remain **provisional** for CATIA until the
  leaf parsers thread `DecodeContext` and emit the intended charges above.
  Freeze the CATIA contribution only after that migration lands, against the
  cumulative profiles measured on the fixtures listed above.
- `object_graph` observed charges on the `object_graph_stream` /
  `object_graph_vm_stream` fixtures: `work` = one whole-image scan pass per
  marker family (three passes over a few hundred bytes) plus each `7C08`
  candidate's `total_len`; `alloc_bytes` = a handful of `grow_vec` element
  charges (two records, their head tokens, and payload fields);
  `retained_bytes` = the single `0xe5` blob payload in record 1 and, for
  `markers_7cd9`, the two bounded `7CD9` context windows. Peak cumulative alloc
  on these fixtures stays in the low hundreds of bytes; the `85` crate tests and
  the `catia_object_graph` fuzz target exercise the charged paths without a
  semantic-hash change. These are the values the section 5.2 freeze should pin
  for the `7C08` family.
- `zero_entity` observed charges on the
  `zero_entity_parser_decodes_face_loop_lanes_and_packed_senses` fixture: `work`
  = one whole-window scan pass over the ~1 KB `a9 03` stream; `alloc_bytes` = a
  `grow_vec` element charge per admitted record (roughly a dozen records:
  carrier, support, face, loop, physical edge, side pair, two coedge twins,
  incidence, vertex marker); `retained_bytes` = the sum of the admitted record
  bodies (each record's framed `YY + 12` extent copied into `record.bytes`).
  Peak cumulative alloc on this fixture stays in the low kilobytes, dominated by
  the retained record copies rather than the reference-lane vectors. The
  truncation fixture (`zero_entity_parser_rejects_record_truncated_at_declared_length`)
  charges one scan pass and admits no record. These are the values the section
  5.2 freeze should pin for the `a9 03` family.
- The acceptance envelope's CATIA structural ratio (doc section 5.2 names the
  64x container figure) is a container-structure default, not a decode-to-IR
  bound; defend it by re-measuring `active_brep` length / root length across
  the golden corpus at freeze time.
