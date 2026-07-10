# cadmpeg

**One open pipeline for native CAD.**

cadmpeg aims to do for CAD files what FFmpeg does for media: provide one open toolchain for reading, inspecting, converting, and building on native formats. It decodes vendor files into a documented intermediate representation (IR), validates the result, and exports neutral formats with an explicit account of what was preserved, approximated, or lost.

cadmpeg is early. The repository implements an end-to-end Fusion 360 `.f3d` to STEP path, while codecs for SolidWorks, CATIA, NX, and Creo provide partial support at different fidelity levels. The long-term goal is one inspectable pipeline for every major CAD format, giving users control of their CAD data and other tools a common foundation to build on.

[Try it](#quick-start) · [Format support](docs/format-support.md) · [Donate a test file](corpus/README.md) · [Contribute](CONTRIBUTING.md)

## Why cadmpeg

Most native CAD formats are proprietary and sparsely documented. Neutral exports such as STEP make geometry portable, but they do not expose the native file or preserve all design data. cadmpeg reads the native bytes directly so users and tools can inspect what was decoded, what was inferred, and what could not be represented.

Four rules shape the project:

- **Common IR.** Every decoder targets the same documented representation. Validators, exporters, and downstream tools build on one interface.
- **Byte traceability.** Decoders distinguish byte-derived values from derived or inferred values. IR values record source offsets where available.
- **Loss accounting.** Every export reports what it carried, approximated, or dropped.
- **Clean-room inputs.** Format knowledge comes from CAD files contributors may legally possess and public information, without vendor SDKs, decompiled binaries, or confidential material. See [LEGAL.md](LEGAL.md).

## Quick start

cadmpeg requires Rust 1.82 or later. Install it from source:

```sh
git clone https://github.com/cadmpeg/cadmpeg
cd cadmpeg
cargo install --path crates/cadmpeg
```

Convert a Fusion 360 file to STEP:

```sh
cadmpeg convert part.f3d -f step -o part.step
```

The conversion reports validation results and any loss:

```text
decode report (f3d): geometry_transferred=true
losses:
  [info/geometry] 22 spline surface record(s) were decoded into NURBS carriers.
  ...
validation: OK (0 error(s), 0 warning(s))
wrote part.step (2125 entities)
```

STEP export is pure Rust and needs no external geometry kernel.

## Format support

The repository contains five native-format codecs:

- **Fusion 360 `.f3d`:** partial B-rep, design-record, and appearance decode.
- **SolidWorks `.sldprt`:** partial semantic read, native write, and round-trip support.
- **Siemens NX `.prt`:** partial analytic, NURBS, trimmed-curve, and topology decode.
- **CATIA V5 `.CATPart`:** partial analytic and freeform decode with conditional B-rep topology.
- **Creo `.prt`:** container and prototype-structure decode with derived datum-plane carriers.

The pure-Rust STEP AP214 writer emits analytic and B-spline manifold B-rep geometry and reports anything it cannot represent.

The [format support profiles](docs/format-support.md) are the authoritative capability breakdown. Per-format specifications in [`docs/formats/`](docs/formats/) define byte semantics; adjacent `*-open-items.md` files track unresolved fields and structures.

## How it works

```text
input file ──▶ container decoder ──▶ format decoder ──▶ IR ──▶ validator ──▶ exporter ──▶ output + loss report
```

The IR connects the pipeline. Decoders produce it, validators check it, and exporters consume it. It serializes to JSON, making a native decode available for inspection and independent tooling.

- [CAD IR](docs/cad-ir.md) defines the representation and its guarantees.
- [Architecture](docs/architecture.md) describes the pipeline, codec interface, and crate map.
- [Format support](docs/format-support.md) records current fidelity by format.
- [Roadmap](docs/roadmap.md) lists planned work and good first issues.

## CLI

```text
cadmpeg inspect  part.f3d
cadmpeg decode   part.f3d -o part.cadir.json
cadmpeg validate part.cadir.json
cadmpeg export   part.cadir.json -f step -o part.step
cadmpeg convert  part.f3d -f step -o part.step
cadmpeg diff     a.cadir.json b.cadir.json
```

`convert` runs decode, validation, and export. `export` writes an IR directly without validating it first. Each stage of **inspect → decode → validate → export** reports its coverage.

Output formats are `cadir` and `step`; `json` remains an alias for `cadir`. When `-f` is omitted, `export` and `convert` infer the format from `.cadir`, `.json`, `.step`, or `.stp` output paths. Use `--input-format` to bypass source-format detection.

Artifact-producing commands write only the artifact to stdout and send diagnostics to stderr. `--report <path>` writes a machine-readable JSON report containing `schema_version: 1`, command details, losses, and decode, validation, or export results where applicable. Existing output and report files require `--force`.

Exit status 0 means success, 1 means a semantic failure or structural difference, and 2 means an operational error. `convert` refuses invalid IR unless `--allow-invalid` is passed and refuses geometry exports with no transferred geometry unless `--allow-empty` is passed.

`diff` compares units, tolerances, and every IR arena by entity ID. IDs are deterministic within one decode, but positional, so comparisons between unrelated source files can be noisy.

## Contributing

Public test files are the most immediate need. The corpus starts empty because the files used to develop the decoders cannot be redistributed. If you can author a CAD file and dedicate it to the public domain under CC0, [donate it to the corpus](corpus/README.md) to give cadmpeg reproducible public coverage.

Code and format contributions are also welcome:

- Implement a codec from a format specification.
- Resolve an open format item with byte-backed evidence.
- Add validators, exporters, IR tooling, corpus tooling, or CLI improvements.

Every commit requires a DCO sign-off. Decoder and specification contributions also require a provenance declaration. Read [CONTRIBUTING.md](CONTRIBUTING.md) for the process, [LEGAL.md](LEGAL.md) for acceptable sources, and [docs/roadmap.md](docs/roadmap.md) for starter work.

## Development

From the repository root:

```sh
cargo build --workspace
cargo test --workspace
```

## Licensing

Code is licensed under the [Apache License 2.0](LICENSE). Documentation and format specifications are licensed under [Creative Commons Attribution 4.0 International](LICENSE-docs). Contributions use the same terms.

SolidWorks, CATIA, Fusion 360, Creo, NX, Parasolid, and other product names are trademarks of their respective owners. cadmpeg uses them only to identify the file formats its decoders target. cadmpeg is an independent project and is not affiliated with, endorsed by, or sponsored by any CAD vendor. See [LEGAL.md](LEGAL.md).
