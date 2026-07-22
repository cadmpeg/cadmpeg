# cadmpeg architecture

cadmpeg routes native CAD containers through format codecs into neutral `CadIr` version 55 plus an independently versioned source-fidelity sidecar, then optionally validates and encodes them. [cad-ir.md](cad-ir.md) defines canonical units and parameterization, identity, topology, free carriers, sidecar annotations, and native-namespace contracts. [byte-accounting.md](byte-accounting.md) defines sidecar accounting. Crate documentation and `cadmpeg --help` define exact APIs and CLI options.

## Pipeline

```text
native CAD ── detect + inspect ──> container summary
     │
     └── detect + decode ──> CadIr ── validate ──> validation report
                                │
                                └── encode ──> .cadir.json | .step/.stp | .f3d | .sldprt
```

- `inspect` detects a codec and calls its container inspection path. It reports streams, blocks, entries, sizes, and codec-defined attributes without decoding geometry.
- `decode` runs the selected codec and serializes `CadIr`, normally as `.cadir.json`.
- `validate` reads or decodes an input and checks IR invariants.
- `export` reads or decodes an input and writes CADIR, STEP, or SLDPRT without validation.
- `convert` performs load/decode, validation, and export. Validation errors stop export unless `--allow-invalid` is set.
- `diff` reads or decodes two inputs and compares units, tolerances, the neutral model, and native namespaces. Source-fidelity annotations and byte ownership use the independent sidecar diff. ID-bearing records are matched by globally unique IDs. Vector position is not entity identity.

CADIR input bypasses codec detection and parses directly into `CadIr`. The parser accepts exactly IR version 55, including its required `subds` arena and excluding source-byte accounting. Library callers can explicitly migrate version 54 with `CadIr::migrate_json`. Geometry exports are refused when a source decode transferred no geometry unless `--allow-empty` is set.

## CLI stream and exit contract

`decode`, `export`, and `convert` reserve stdout for the output artifact; diagnostics use stderr. `--report <path>` writes a machine-readable command report with `schema_version: 3`, including semantic refusal paths. JSON output from `inspect`, `validate`, and `diff` uses the same CLI schema version. This envelope version is independent of `CadIr.ir_version`. Status 0 means success, status 1 means semantic failure or a non-empty diff, and status 2 means operational failure.

Output and report files are written through a unique temporary file in the destination directory and then persisted. Existing files require `--force`. An output path resolving to the input is rejected.

## Loss reports

Source decoders return `DecodeReport`, including `geometry_transferred`, a decode-coverage count census, notes, and attributable `LossNote` entries. Validation propagates supplied decode losses unchanged.

Every encoder returns an `ExportReport` containing its format id, entity census, total entity count, loss notes, and informational notes. STEP reports reductions and omitted IR data. CADIR has no export losses. F3D and SLDPRT retain all-or-nothing rejection for unsupported input and report whether the source container was replayed or regenerated. Decode losses remain present in the command report when export or convert started from native CAD.

The [format support profiles](format-support.md) record read, write, and round-trip capability by semantic domain, summarized as one [ladder score](format-support.md#support-ladder) per codec.

## Crate map

| Crate                  | Responsibility                                                                                                                                                                                                 |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cadmpeg`              | CLI orchestration for `inspect`, `decode`, `validate`, `export`, `diff`, and `convert`; built-in codec registration; CADIR, STEP, and SLDPRT output dispatch.                                                  |
| `cadmpeg-ir`           | Layered version 55 IR model, canonical JSON, free-carrier source associations, source-fidelity sidecars, sparse provenance and exactness, native namespaces, structural diff, validation, codec traits, and report types. |
| `cadmpeg-codec-f3d`    | `.f3d` ZIP inspection; ASM/SAB B-rep, analytic and cached NURBS geometry, pcurves, transforms, attributes, appearances, Design/ACT records, history decode, retained-source replay, and selected native edits. |
| `cadmpeg-codec-sldprt` | SLDPRT block, directory, and cache-cell inspection; Parasolid analytic/NURBS B-rep, pcurves, appearances, feature lanes, history, and tessellation decode; retained-source and semantic SLDPRT writing.        |
| `cadmpeg-codec-catia`  | CATIA V5 `V5_CFV2` layout inspection; standard, zero-entity, E5, and object-stream carrier decode; conditional standard-nested topology reconstruction.                                                        |
| `cadmpeg-codec-nx`     | NX `SPLMSSTR` extraction; Parasolid analytic and NURBS carriers, supported trimmed-curve bindings, and conditional topology reconstruction.                                                                    |
| `cadmpeg-codec-creo`   | Creo `#UGC:2`/PSB section and token decode, prototype and loop structure, opaque `VisibGeom` preservation, placed plane and selected cylinder carriers, and conditional planar model B-rep transfer.           |
| `cadmpeg-step`         | Pure-Rust STEP AP214 writer for supported B-rep hierarchy, analytic, and B-spline carriers, with export loss notes.                                                                                            |
| `cadmpeg-fuzz`         | `cargo-fuzz` targets and seed generators for untrusted decoder inputs. It is excluded from the default Cargo workspace because libFuzzer requires nightly; run it with `cargo +nightly fuzz ...`.              |

## Codec interface

Each input codec implements `Codec`:

- `id() -> &'static str` names the codec for registry lookup and `--input-format`.
- `detect(&[u8]) -> Confidence` identifies a format from a byte prefix.
- `inspect(&mut dyn ReadSeek) -> Result<ContainerSummary, CodecError>` enumerates container structure.
- `decode(&mut dyn ReadSeek, &DecodeOptions) -> Result<DecodeResult, CodecError>` produces `CadIr` and `DecodeReport`.

The CLI detects a codec unless `--input-format` selects one. Output codecs that implement native encoding use the separate `Encoder` trait. The Rust trait definitions are authoritative for exact signatures.

## Build shape

The default workspace and the core inspect/decode/validate/export path build on stable Rust without native CAD SDKs. `cadmpeg-fuzz` is an explicit nightly `cargo-fuzz` workflow outside the default workspace.
