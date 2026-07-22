# cadmpeg

**One open pipeline for native CAD.**

cadmpeg aims to do for CAD what FFmpeg does for media: provide one open toolchain for reading, inspecting, converting, and building across formats. It decodes vendor files into a documented intermediate representation (IR), validates them, and exports neutral formats.

cadmpeg is early. End-to-end Autodesk Fusion `.f3d` to STEP path is about 70% complete, while codecs for SolidWorks, Rhino, CATIA, NX, and Creo cover different subsets. Long-term goal is one inspectable pipeline for every major CAD format.

[Try it](#quick-start) · [Format support](docs/format-support.md) · [Donate a test file](corpus/README.md) · [Contribute](CONTRIBUTING.md)

## Why cadmpeg

Native CAD formats are proprietary and sparsely documented. Neutral formats such as STEP make geometry portable but discard design data.

Every decoder writes to one documented IR used by validators, exporters, and downstream tools. Values retain source byte offsets, inferred values are marked, and unsupported content is reported as loss.

Format knowledge comes from legally possessed CAD files and public documentation. Vendor SDKs, decompiled binaries, and confidential material are prohibited ([LEGAL.md](LEGAL.md)).

The goal is high-fidelity conversion across formats, versions, and vendors, including parametric design history.

## Install

Build from source with Rust 1.88 or later:

```sh
git clone https://github.com/cadmpeg/cadmpeg
cd cadmpeg
cargo install --path crates/cadmpeg
```

Homebrew (macOS):

```sh
brew install cadmpeg/tap/cadmpeg
```

Installer script (macOS, Linux):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/cadmpeg/cadmpeg/releases/latest/download/cadmpeg-installer.sh | sh
```

Windows:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/cadmpeg/cadmpeg/releases/latest/download/cadmpeg-installer.ps1 | iex"
```

## Quick start

```sh
cadmpeg convert part.f3d -f step
```

Conversion reports validation results and loss:

```text
decode report (f3d): geometry_transferred=true
losses:
  [info/geometry] 22 spline surface record(s) were decoded into NURBS carriers.
  ...
validation: OK (0 error(s), 0 warning(s))
wrote part.step (2125 entities)
```

## Format support

The repository contains six native-format codecs:

- **Autodesk Fusion `.f3d` — [L4](docs/format-support.md#support-ladder):** readable design records; partial B-rep and appearance decode; native replay, patching, and generation.
- **SolidWorks `.sldprt` — [L4](docs/format-support.md#support-ladder):** connected model read; typed design records; native write and round-trip paths.
- **Rhino `.3dm` — [L9](docs/format-support.md#support-ladder) for archive 50/60/70/80:** complete built-in model, product structure, presentation, annotations, application-data retention, and bounded semantic native writing; older bands have inspection and metadata support.
- **Siemens NX `.prt` — [L4](docs/format-support.md#support-ladder) for single-body, `RMFastLoad`-selected, and terminal-feature-lineage-resolved body images; L2 for unresolved multi-partition history:** exact carriers, connected B-rep, ordered feature history, expressions, and typed sketch-point dependencies, with conservative retention when final-body membership remains inconclusive.
- **CATIA V5 `.CATPart` — [L2](docs/format-support.md#support-ladder):** exact carriers with conditional topology on the standard-nested layout; other layouts at L1.
- **Creo `.prt` — [L1](docs/format-support.md#support-ladder):** structural decode with partial placed geometry, topology, sketches, and design records.
- **STEP Part 21 AP203/AP214/AP242 — [L9](docs/format-support.md#support-ladder):** full-document read and semantic clear-text write with target-schema selection and strict fidelity checks.

The pure-Rust STEP writer re-decodes generated files and can reject every reported semantic loss before writing.

[Format support profiles](docs/format-support.md) detail current capabilities. [`docs/formats/`](docs/formats/) defines byte semantics and tracks unresolved fields and structures.

## Pipeline

```text
input file ──▶ container decoder ──▶ format decoder ──▶ IR ──▶ validator ──▶ exporter ──▶ output + reports
```

The IR connects the pipeline. Decoders produce it, validators check it, and exporters consume it. Version 2 serializes a format-neutral model, the required `subds` control-cage arena, free-carrier source associations, sparse source annotations, independently versioned native namespaces, and opaque records as canonical JSON. Arena entries are ordered by ID after finalization, and carrier reachability follows topology links, procedural references, and source associations.

- [CAD IR version 55](docs/cad-ir.md) defines byte semantics, canonical units and parameterization, identity, topology, directed SubD control cages, bounded procedural constructions, annotations, native opacity, and versioning.
- [Architecture](docs/architecture.md) describes the pipeline, codec interface, and crate map.
- [Format support](docs/format-support.md) records current capability by format.
- [Roadmap](docs/roadmap.md) defines milestones and contributor entry points.

## CLI

```text
cadmpeg inspect  part.f3d
cadmpeg decode   part.f3d -o part.cadir.json
cadmpeg validate part.cadir.json
cadmpeg export   part.cadir.json -f step -o part.step
cadmpeg convert  part.f3d -f step -o part.step
cadmpeg diff     a.cadir.json b.cadir.json
```

Output formats are `cadir`, `step`, `fcstd`, `f3d`, and `sldprt`; `json` aliases `cadir`. `export` and `convert` infer omitted formats from the output extension. Use `--input-format` to override source detection.

Machine-readable output from `inspect --json`, `validate --json`, and `diff --json`, plus command report files, uses CLI `schema_version: 3`. This command-envelope version is independent of the CAD IR's `ir_version: "5"`.

## Contributing

Public test files are the most immediate need. If you can dedicate a CAD file to the public domain under CC0, please [donate it to the corpus](corpus/README.md).

Other contributions:

- Implement a codec from a format specification.
- Resolve an open format item with byte-backed evidence.
- Add validators, exporters, IR tooling, corpus tooling, or CLI improvements.

Commits require DCO sign-off; decoder and specification changes also require a provenance declaration. See [CONTRIBUTING.md](CONTRIBUTING.md), [LEGAL.md](LEGAL.md), and the [roadmap](docs/roadmap.md).

## Development

From the repository root:

```sh
cargo build --workspace
cargo test-fast
```

Run an end-to-end smoke test:

```sh
cargo run -p cadmpeg-ir --example emit_cube > cube.cadir.json
cadmpeg export cube.cadir.json -f step -o cube.step
```

AI-assisted contributions are welcome when reviewed and concise. Clean-room rules still apply: do not pass vendor SDK knowledge through a model.

## Licensing

Code uses the [Apache License 2.0](LICENSE); documentation and specifications use [CC BY 4.0](LICENSE-docs). Contributions use the corresponding license.

SolidWorks, Rhino, CATIA, Autodesk Fusion, Creo, NX, Parasolid, and other product names are trademarks of their respective owners. cadmpeg uses them only to identify the file formats its decoders target. cadmpeg is an independent project and is not affiliated with, endorsed by, or sponsored by any CAD vendor. See [LEGAL.md](LEGAL.md).
