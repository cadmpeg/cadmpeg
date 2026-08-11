# Fuzz-target consumers of ungated codec APIs

Inventory of `crates/cadmpeg-fuzz` imports from codec crates that do not gate
internals behind a `fuzzing` feature (inventor, rhino, sldprt, and catia do).
Built from `use cadmpeg_codec_*::…` imports and path-qualified
`cadmpeg_codec_*::…` references under `fuzz_targets/` and `src/`.

Verdict:

- `deliberately-public` — crate-level `//!` docs advertise the item (quoted).
- `fuzz-only` — public today, not advertised in crate-level docs; candidate for
  a future `fuzzing` feature gate.

No visibility changes accompany this table.

Gated codecs (inventor, rhino, sldprt, catia) are omitted. `cadmpeg-codec-sat`
has no fuzz-target references.

Shared target `decode_pipeline_mutated` appears under each ungated codec it
constructs.

---

## creo (`cadmpeg-codec-creo`)

Crate docs advertise low-level modules:

> `[container]` identifies ND and DEPDB layouts… `[psb]` and `[scalar]` expose
> the context-independent primitive decoders. `[surface]`, `[curve]`,
> `[reference]`, `[primdata]`, `[feature]`, and `[topology]` expose the typed
> structural model.

| fuzz target | item used | verdict |
| --- | --- | --- |
| `creo_container` | `CreoCodec` | deliberately-public — "`[CreoCodec]` implements `[cadmpeg_ir::codec::Codec]`" |
| `decode_pipeline_mutated` | `CreoCodec` | deliberately-public — same |
| `creo_container_scan` | `container::scan_bytes` | deliberately-public — "`[container]` identifies ND and DEPDB layouts…" |
| `creo_compact_int` | `psb::compact_int` | deliberately-public — "`[psb]` … expose the context-independent primitive decoders" |
| `creo_psb_tokens` | `psb::tokens` | deliberately-public — same |
| `creo_short_form_float` | `psb::short_form_float` | deliberately-public — same |
| `creo_scalar` | `scalar::{decode, decode_in_lane, ScalarCache}` | deliberately-public — "`[scalar]` expose the context-independent primitive decoders" |
| `creo_surface_rows` | `surface::rows` | deliberately-public — "`[surface]` … expose the typed structural model" |
| `creo_curve_prototypes` | `curve::prototypes` | deliberately-public — "`[curve]` … expose the typed structural model" |
| `creo_datum` | `datum::{named_plane, planes}` | fuzz-only |

---

## f3d (`cadmpeg-codec-f3d`)

| fuzz target | item used | verdict |
| --- | --- | --- |
| `f3d_container` | `F3dCodec` | deliberately-public — "`[F3dCodec]` implements `[Codec]` and `[Encoder]`" |
| `f3d_writer` | `F3dCodec` | deliberately-public — same |
| `f3d_roundtrip` | `F3dCodec` | deliberately-public — same |
| `decode_pipeline_mutated` | `F3dCodec` | deliberately-public — same |

---

## freecad (`cadmpeg-codec-freecad`)

| fuzz target | item used | verdict |
| --- | --- | --- |
| `fcstd_container` | `FcstdCodec` | deliberately-public — "`[FcstdCodec]` implements `[Codec]` and `[Encoder]`" |
| `fcstd_decode` | `FcstdCodec` | deliberately-public — same |
| `fcstd_support` (shared module) | `FcstdCodec` | deliberately-public — same |
| `fcstd_write` | `FcstdCodec` | deliberately-public — same |
| `fcstd_write` | `FcstdPropertyOwner` | fuzz-only |
| `decode_pipeline_mutated` | `FcstdCodec` | deliberately-public — "`[FcstdCodec]` implements `[Codec]` and `[Encoder]`" |

Focused FCStd harnesses (`fcstd_xml`, `fcstd_gui`, `fcstd_brep`,
`fcstd_element_map`, `fcstd_auxiliary`) import `FcstdCodec` only through
`fcstd_support`.

---

## iges (`cadmpeg-codec-iges`)

Crate-level docs name no public Rust items (they describe Fixed ASCII versions
and support level only).

| fuzz target | item used | verdict |
| --- | --- | --- |
| `iges_container` | `IgesCodec` | fuzz-only |
| `iges_writer` | `IgesCodec` | fuzz-only |
| `iges_writer` | `IgesEncoder` | fuzz-only |
| `iges_writer` | `IgesVersion` | fuzz-only |
| `iges_writer` | `IgesWriteOptions` | fuzz-only |

---

## nx (`cadmpeg-codec-nx`)

Crate docs advertise low-level modules and bound the object-model tier:

> The public submodules expose the lower-level container, stream, geometry,
> NURBS, intersection, and topology decoders. The object-model extraction and
> attachment tier … is crate-internal …

| fuzz target | item used | verdict |
| --- | --- | --- |
| `nx_container` | `NxCodec` | deliberately-public — "Applications that need a complete IR entry point should use `[NxCodec]`" |
| `decode_pipeline_mutated` | `NxCodec` | deliberately-public — same |
| `nx_parasolid` | `container::scan_bytes` | deliberately-public — "public submodules expose the lower-level container …" |
| `nx_parasolid` | `parasolid::extract_streams` | deliberately-public — "… container, stream, geometry, NURBS, intersection, and topology decoders" |
| `nx_deltas` | `deltas::walk` | deliberately-public — same (`stream`) |
| `nx_geometry_points` | `geometry::points` | deliberately-public — "… geometry …" |
| `nx_geometry_curves` | `geometry::curves` | deliberately-public — same |
| `nx_geometry_surfaces` | `geometry::surfaces` | deliberately-public — same |
| `nx_nurbs_curves` | `nurbs::curves` | deliberately-public — "… NURBS …" |
| `nx_nurbs_surfaces` | `nurbs::surfaces` | deliberately-public — same |
| `nx_intersection` | `intersection::{curves, ChartPointLayout}` | deliberately-public — "… intersection …" |
| `nx_topology` | `topology::{Graph::parse, composite_curves, intersection_data_curves, blend_surfaces, offset_surfaces, surface_curves, trimmed_curves}` | deliberately-public — "… and topology decoders" |
| `nx_om` | `om::indexed_sections` | fuzz-only — "object-model extraction … is crate-internal" |

---

## step (`cadmpeg-codec-step`)

| fuzz target | item used | verdict |
| --- | --- | --- |
| `step_writer` | `write_step` | deliberately-public — "`[write_step]` emits the application protocol selected by `[StepWriteOptions::schema]`" |
| `step_writer` | `StepWriteOptions` | deliberately-public — same |
| `step_writer_custom` | `write_step`, `StepWriteOptions` | deliberately-public — same |
| `step_geometry_degenerate` | `write_step`, `StepWriteOptions` | deliberately-public — same |
| `step_decode` | `StepCodec` | fuzz-only |
| `step_reader` | `StepCodec` | fuzz-only |
| `step_lexer` | `lex::lex` | fuzz-only |
| `step_parser` | `parse::parse` | fuzz-only |

---

## sat (`cadmpeg-codec-sat`)

No `cadmpeg_codec_sat` references under `crates/cadmpeg-fuzz`.
