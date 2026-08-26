# cadmpeg architecture

cadmpeg routes native CAD through format codecs into `CadIr` version 5. Source fidelity rides beside the product document as a sidecar. Codecs may validate and encode afterward. [cad-ir.md](cad-ir.md) defines the IR. Crate docs and `cadmpeg --help` define APIs and CLI options. [format-support.md](format-support.md) records per-envelope capability.

## Pipeline

```text
native CAD ── detect + inspect ──> container summary
     │
     └── detect + decode ──> CadIr ── check ──> check report
                                │
                                └── dump ──> .cadir.json | .step/.stp | .f3d | .sldprt | .3dm | .FCStd
```

- `convert` loads or decodes, checks, then writes another format. `--allow-errors` writes after check errors.
- `inspect` detects a codec and reports container structure without decoding geometry.
- `diff` reads or decodes two inputs and compares units, tolerances, the neutral model, native namespaces, source metadata, source annotations, and retained records. ID-bearing records match by globally unique IDs. Vector position is not entity identity. A source attribute whose key ends in `_local_sha256` holds a machine-local content digest, which no two platforms reproduce; `diff` reports a difference in one under a separate informational section and keeps status 0.
- `check` reads or decodes an input and checks IR invariants. Decoder and export admission subsets are in [admissibility-routes.md](admissibility-routes.md).
- `dump` runs the selected codec and serializes `CadIr`, normally as `.cadir.json`.

CADIR input parses directly into `CadIr`. The parser accepts exactly IR version 5, including its required `subds` arena. Source annotations and retained records stay in the source-fidelity sidecar. `--allow-empty` permits geometry export when a source decode transferred no geometry.

A successful dump is not a checked model.

## Decode session

The safe consumer trait is `Codec` (`inspect` / `decode`). Format crates implement the raw hook trait `CodecBackend` (`inspect_impl` / `decode_impl`). The `Codec` blanket wrapper acquires the root input under `DecodePolicy` limits, records the container-only request, runs the backend, and finalizes a `DecodeContext`. Its strict gate evaluates full-decode reports only: a container-only report keeps its losses and is never refused by that gate.

`DecodeContext` holds budget counters and the address-space registry. `DecodeArena` holds byte buffers with stable addresses. A `Copy` `View` carries bounded, space-tagged navigation. `DecodeOptions` carries a `policy` field. Ownership lives in `cadmpeg_core::decode`. Classify a named `MAX_*` cap with [decode-resource-caps.md](decode-resource-caps.md) before adding a bound or a `ResourceLimits` field.

Semantic decode is bounded by the caller's `DecodePolicy`. The policy limits input bytes, temporary materialization, retained bytes, admitted entities, collection items, recursive depth, and algorithm work. File-declared counts, pointer walks, recursive definitions, and geometric recovery cannot override these limits. A refused request returns a structured resource refusal and does not return a partial semantic document. The policy is implementation admission control; no source field changes it.

## CLI stream and exit contract

`dump` and `convert` reserve stdout for the output artifact. Diagnostics use stderr. `--report <path>` writes a machine-readable command report with `schema_version: 6` with top-level `status` (`ok` | `refused`) and `refusal` (`{ stage, code, message }` or null), including semantic refusal paths. A codec-level decode failure during `dump` with an explicit report is a `decode`-stage `decode_failed` refusal; an I/O failure remains an operational exit. JSON from `inspect`, `check`, and `diff` uses the same CLI schema version. That envelope version is independent of `CadIr.ir_version`.

Status 0 is success. Status 1 is a negative verdict on a verdict command; other commands stay off 1. Status 2 is operational failure.

Writers create a unique temporary file in the destination directory, then rename it into place. `--force` replaces an existing file. The CLI rejects an output path that resolves to the input.

## Loss reports

Source decoders return `DecodeReport`, including `geometry_transferred`, a decode-coverage census, notes, and attributable `LossNote` entries. Validation propagates supplied decode losses unchanged.

Each codec owns a `*LossCode` enum in `src/loss.rs`. Every reported drop goes through `code.note(message)`, which pins the namespaced local code, `LossTaxonomy`, severity, and strict floor. Shared taxonomy is the category used for subsystem reporting; the stable machine-readable identifier is the codec-local `family.detail` string.

Every encoder returns an `ExportReport` with its format id, entity census, loss notes, informational notes, and a `write_path`. STEP reports reductions and omitted IR data. CADIR export carries no losses. F3D, SLDPRT, Rhino, and FreeCAD report replay versus regeneration and reject unsupported input atomically. Decode losses remain in the command report when convert started from native CAD.

`write_path` names which of an encoder's write paths produced the bytes: `verbatim_replay` copies retained source bytes out unchanged, `patched` runs the writer over retained source content, and `synthesized` runs the writer over neutral IR content alone. The encoder sets it at the branch it takes. The distinction is not recoverable from the output, because a patch that changes nothing observable reproduces its input byte for byte. F3D takes all three paths, SLDPRT takes all three, FreeCAD is always `patched`, and CADIR, STEP, and Rhino are always `synthesized`.

Export-side refusal has two owners, and the `Encoder` trait is neither. The conversion layer owns `--reject-lossy`: the application transcoder refuses to plan when the decode report carries any loss and refuses to write when the planned `ExportReport` carries any loss. Both are policy stops distinct from a planning failure, and neither consults per-loss strict floors — any loss note refuses. Separately, a writer may own an unsupported policy of its own: the STEP writer's `StepUnsupportedPolicy` either emits the representable subset with loss notes (the default) or rejects the document before any output byte when its report holds a loss, and the atomic-rejection encoders above refuse unsupported input the same way. A format specification's "strict export rejects" sentence names the owning writer policy where one exists and the conversion stop otherwise.

## Crate map

| Crate                    | Responsibility                                                                                                                         |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `cadmpeg`                | CLI orchestration, built-in codec registration, and output dispatch.                                                                   |
| `cadmpeg-ir`             | `CadIr` version 5, validation, diff, codec traits, reports, and source-fidelity sidecars.                                              |
| `cadmpeg-core`           | Shared decode budgets, arenas, views, container summaries, and I/O helpers.                                                            |
| `cadmpeg-container`      | Shared archive and compression helpers for container codecs.                                                                           |
| `cadmpeg-protein`        | Shared schema and paged instance-property decoding for Protein asset packages.                                                         |
| `cadmpeg-codec-freecad`  | FreeCAD `.FCStd` read and semantic write for the schema-4/file-1 envelope.                                                             |
| `cadmpeg-codec-f3d`      | Fusion `.f3d` inspection, ASM/SAB geometry, design records, retained replay, and selected native edits.                                |
| `cadmpeg-codec-inventor` | Inventor `.ipt`/`.iam` compound, RSe, part geometry, external occurrences, properties, appearances, and design-record decode.          |
| `cadmpeg-codec-sldprt`   | SolidWorks `.sldprt` container, Parasolid B-rep, features, retained replay, and semantic writing.                                      |
| `cadmpeg-codec-rhino`    | Rhino `.3dm` read and write for archive 50/60/70/80.                                                                                   |
| `cadmpeg-codec-catia`    | CATIA V5 `.CATPart` layout inspection and carrier decode; conditional topology on the standard-nested band.                            |
| `cadmpeg-codec-nx`       | NX `.prt` `SPLMSSTR` extraction, Parasolid carriers, and conditional topology.                                                         |
| `cadmpeg-codec-creo`     | Creo `.prt` section decode with partial placed geometry and conditional connected bodies (general analytic intersections and pcurves). |
| `cadmpeg-codec-sat`      | Bare ASM/ACIS `.sat`/`.smt`/`.smb`/`.sab` stream inspection and B-rep transfer outside any container.                                  |
| `cadmpeg-codec-iges`     | IGES 5.1/5.2/5.3 Fixed ASCII read and bounded semantic write for the mechanical/document envelope.                                     |
| `cadmpeg-codec-step`     | STEP Part 21 AP203, AP214, and AP242 read and write with export loss notes.                                                            |
| `cadmpeg-fuzz`           | Nightly `cargo-fuzz` targets outside the default workspace.                                                                            |

## Codec interface

Each input codec implements `Codec`:

- `id() -> &'static str` names the codec for registry lookup and `--input-format`.
- `detect(&[u8]) -> Confidence` identifies a format from a byte prefix.
- `inspect(&mut dyn ReadSeek) -> Result<ContainerSummary, CodecError>` enumerates container structure.
- `decode(&mut dyn ReadSeek, &DecodeOptions) -> Result<DecodeResult, CodecError>` produces `CadIr`, `DecodeReport`, and source fidelity.

`--input-format` selects a codec. Without it, the CLI detects one. Native writers use the separate `Encoder` trait. The Rust trait definitions are authoritative for exact signatures.

Strict decode refuses at the `Codec::decode` wrapper, which returns `CodecError::StrictRefusal` for the first reported loss whose strict consequence is `Reject`. The wrapper applies that predicate to full-decode reports only: a container-only decode keeps its losses and is admitted in either mode. The refusal carries that loss code and that loss message. It is not `CodecError::Malformed`: a strict refusal reports a mode decision, not a defect in the bytes, so a caller separates a damaged file from a policy stop by the error class alone. A codec reports its losses with their strict floors and adds no strict gate of its own. A local gate widens the refusal predicate and reclassifies the refusal where the caller cannot see it.
