# cadmpeg architecture

cadmpeg uses a fixed decode pipeline: container decoder, format decoder, IR, validator, exporter. Subcommands enter and exit that pipeline at different stages.

[cad-ir.md](cad-ir.md) defines the IR schema. Crate documentation and `cadmpeg --help` define exact CLI flags and feature names.

---

## The pipeline

```
                 ┌──────────────────────────────────────────────────────────────┐
                 │                           cadmpeg                            │
                 └──────────────────────────────────────────────────────────────┘

  input file
  (.sldprt,        ┌───────────┐      ┌──────────┐      ┌──────┐      ┌───────────┐      ┌──────────┐
   .f3d,     ─────▶│ container │─────▶│  format  │─────▶│  IR  │─────▶│ validator │─────▶│ exporter │────▶ output
   .prt, …)        │  decoder  │      │  decoder │      │      │      │           │      │          │      (.step,
                   └───────────┘      └──────────┘      └──┬───┘      └─────┬─────┘      └────┬─────┘      .json)
                    unwrap the         native bytes    inspectable   consistency        neutral
                    on-disk wrapper    -> IR nodes      byte-traced   checks             format         +
                    (streams, blocks,  with exactness   model                            emit           ┌──────────┐
                    entities)          labels                                                           │   loss   │
                                                                                                        │  report  │
                                                                                                        └──────────┘

   inspect ─────────┴──────────────────┘
   (stops after reporting structure)

   decode  ─────────┴───────────────────────────────────┘
   (emits IR: *.cadir.json)

   validate ────────────────────────────────────────────────────────┘
   (reads IR, checks it)

   export / convert ─────────────────────────────────────────────────────────────────────────┘
   (IR -> neutral format + loss report)
```

- **`inspect`** runs the container decoder (and a shallow format probe) and reports what is inside (containers, streams, blocks, entity tables) without committing to a full decode.
- **`decode`** runs the full format decoder and emits the IR, typically to a `*.cadir.json` file you can read directly.
- **`validate`** takes IR and checks it for internal consistency.
- **`export`** takes IR or a source file and emits a neutral format without validation.
- **`convert`** runs decode → validate → export and refuses to export validation errors unless `--allow-invalid` is set.
- **`diff`** decodes or reads two inputs and compares units, tolerances, and all entity arenas. IDs are positional, so unrelated source files produce noisy diffs.

## CLI stream and exit contract

`decode`, `export`, and `convert` reserve stdout for the output artifact. Their reports and diagnostics use stderr. `--report <path>` writes a versioned machine-readable report, including on semantic refusal paths. `inspect`, `validate`, and `diff` produce reports on stdout and accept `--json` for versioned machine-readable output. Status 0 means success, status 1 means a semantic failure such as invalid IR, refused empty geometry, or a structural difference, and status 2 means an operational error. Output and report files are written through a unique temporary file in the destination directory and atomically persisted. Existing files require `--force`; a path that resolves to the input is always rejected.

---

## Explicit loss accounting

cadmpeg records conversion loss during decode and export:

- Decoding reports coverage: how much of the input was named and typed versus left as opaque bytes.
- Each export produces a **loss report** stating what was carried across exactly, what was approximated (e.g. an analytic surface tessellated to a mesh), and what was dropped (e.g. parametric feature history when exporting to a geometry-only target).
- The [format support profiles](format-support.md) track read, write, and round-trip capability by semantic domain.

Missing loss accounting is a defect.

---

## Crate map

cadmpeg is a Cargo workspace:

| Crate                  | Responsibility                                                                                                                                                                          |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cadmpeg`              | The CLI binary: `inspect`, `decode`, `validate`, `export`, `diff`, and the `convert` sugar. Orchestrates the pipeline; owns no format knowledge itself.                                 |
| `cadmpeg-ir`           | The intermediate representation: the data model every decoder emits and every validator/exporter consumes. Includes exactness labeling and byte-provenance. See [cad-ir.md](cad-ir.md). |
| `cadmpeg-codec-f3d`    | The `.f3d` codec: SAB framing, partial B-rep topology, analytic and cached NURBS carriers, Design/ACT records, attributes, and appearance bindings.                                     |
| `cadmpeg-codec-sldprt` | The `.sldprt` codec: CRC-validated framing, partial analytic/NURBS B-rep and semantic lanes, retained-source writing, native regeneration, and tessellation.                            |
| `cadmpeg-codec-catia`  | The `.CATPart` codec: `V5_CFV2` and related layout detection, partial analytic/freeform carrier decode, and conditional standard-nested topology.                                       |
| `cadmpeg-codec-nx`     | The NX `.prt` codec: SPLMSSTR extraction, analytic and NURBS carriers, supported trimmed curves, and conditional topology reconstruction.                                               |
| `cadmpeg-codec-creo`   | The Creo `.prt` codec: `#UGC:2` sections, PSB token and prototype-structure decoding, opaque VisibGeom preservation, and derived ActDatums plane carriers.                              |
| `cadmpeg-step`         | Pure-Rust STEP (AP214) writer: manifold B-rep with analytic and B-spline carriers, explicit loss report. No native dependencies.                                                        |
| `cadmpeg-fuzz`         | Fuzz targets for the decoders. The decoders read untrusted binary input, so fuzzing is part of the core workflow.                                                                       |

Every codec crate implements the codec interface described below.

---

## The codec plugin model

Every format decoder implements a common `Codec` trait. The CLI dispatches through that trait. The trait has three responsibilities:

- **`detect`**: given bytes (or a path), decide whether this codec claims the file. `inspect` and `decode` use this method to route an input without a user-declared format.
- **`inspect`**: report the file's structure (containers, streams, entity tables, sizes, offsets) without performing a full geometric decode. Cheap, and safe to run on files the codec only partially understands.
- **`decode`**: perform the full decode and emit `cadmpeg-ir`. The decoder emits only byte-derived values; `Exactness` provenance marks derived or inferred fields.

The Rust trait definition is authoritative for exact signatures. Add a new format by implementing this trait in a new `cadmpeg-codec-*` crate and registering it with the CLI.

---

## Build shape

The core loop (`inspect` / `decode` / `validate` / `export`) has no native dependencies.
