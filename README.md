# cadmpeg

**One open pipeline for native CAD.**

cadmpeg aims to do for CAD files what FFmpeg does for media: provide one open toolchain for reading, inspecting, converting, and building on all CAD formats. It decodes vendor files into a documented intermediate representation (IR), validates the result, and exports neutral formats.

cadmpeg is early. End-to-end Autodesk Fusion `.f3d` to STEP path is about 70% complete, while codecs for SolidWorks, Rhino, CATIA, NX, and Creo cover different subsets. Long-term goal is one inspectable pipeline for every major CAD format.

[Try it](#quick-start) · [Format support](docs/format-support.md) · [Donate a test file](corpus/README.md) · [Contribute](CONTRIBUTING.md)

## Why cadmpeg

Most native CAD formats are proprietary and sparsely documented. Neutral exports such as STEP make geometry portable but drop design data and hide the native file.

Every cadmpeg decoder writes to one documented IR. Validators, exporters, and downstream tools build against that single interface. IR values record the byte offsets they came from, inferred values are marked inferred, and whatever a decoder or exporter cannot carry through it reports as loss.

Format knowledge comes from CAD files contributors may legally possess and from public documentation. Vendor SDKs, decompiled binaries, and confidential material are off limits ([LEGAL.md](LEGAL.md)).

The grand vision is high fidelity conversion between all CAD formats, including parametric design history, across versions and vendors. Impossible to perfectly convert but the idea is to get as close as possible.

## Install

cadmpeg requires Rust 1.88 or later:

```sh
git clone https://github.com/cadmpeg/cadmpeg
cd cadmpeg
cargo install --path crates/cadmpeg
```

Prebuilt binaries are also available. Homebrew (macOS):

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

The conversion reports validation results and any loss:

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
- **SolidWorks `.sldprt` — [L3](docs/format-support.md#support-ladder):** connected model read; native write and round-trip paths.
- **Rhino `.3dm` — [L3](docs/format-support.md#support-ladder) for archive 50/60/70/80:** curves, surfaces, meshes, connected B-rep, SubD, extrusions, and expanded instances; older bands have inspection and metadata support.
- **Siemens NX `.prt` — [L2](docs/format-support.md#support-ladder):** exact carriers with conditional topology.
- **CATIA V5 `.CATPart` — [L2](docs/format-support.md#support-ladder):** exact carriers with conditional topology on the standard-nested layout; other layouts at L1.
- **Creo `.prt` — [L1](docs/format-support.md#support-ladder):** container mastered; no placed model geometry.

The pure-Rust STEP AP214 writer emits supported analytic and B-spline B-rep geometry and reports omitted or reduced IR content.

The [format support profiles](docs/format-support.md) are the authoritative capability breakdown. Per-format specifications in [`docs/formats/`](docs/formats/) define byte semantics; adjacent `*-open-items.md` files track unresolved fields and structures.

## Pipeline

```text
input file ──▶ container decoder ──▶ format decoder ──▶ IR ──▶ validator ──▶ exporter ──▶ output + reports
```

The IR connects the pipeline. Decoders produce it, validators check it, and exporters consume it. Version 2 serializes a format-neutral model, the required `subds` control-cage arena, free-carrier source associations, sparse source annotations, independently versioned native namespaces, and opaque records as canonical JSON. Arena entries are ordered by ID after finalization, and carrier reachability follows topology links, procedural references, and source associations.

- [CAD IR version 2](docs/cad-ir.md) defines byte semantics, canonical units and parameterization, identity, topology, directed SubD control cages, bounded procedural constructions, annotations, native opacity, and versioning.
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

Output formats are `cadir`, `step`, `f3d`, and `sldprt`; `json` remains an alias for `cadir`. When `-f` is omitted, `export` and `convert` infer the format from `.cadir`, `.json`, `.step`, `.stp`, `.f3d`, or `.sldprt` output paths. Use `--input-format` to bypass source-format detection.

Machine-readable output from `inspect --json`, `validate --json`, and `diff --json`, plus command report files, uses CLI `schema_version: 2`. This command-envelope version is independent of the CAD IR's `ir_version: "2"`.

## Contributing

Public test files are the most immediate need. If you can author a CAD file and dedicate it to the public domain under CC0, [donate it to the corpus](corpus/README.md) it would be greatly appreciated!

Code and format contributions are also welcome:

- Implement a codec from a format specification.
- Resolve an open format item with byte-backed evidence.
- Add validators, exporters, IR tooling, corpus tooling, or CLI improvements.

Every commit requires a DCO sign-off. Decoder and specification contributions also require a provenance declaration. Read [CONTRIBUTING.md](CONTRIBUTING.md) for the process, [LEGAL.md](LEGAL.md) for acceptable sources, and [docs/roadmap.md](docs/roadmap.md) for contributor entry points.

## Development

From the repository root:

```sh
cargo build --workspace
cargo test --workspace
```

Run an end-to-end smoke test without a native CAD file:

```sh
cargo run -p cadmpeg-ir --example emit_cube > cube.cadir.json
cadmpeg export cube.cadir.json -f step -o cube.step
```

AI-assisted contributions are welcome but please keep them concise and review the output before submission. The same clean-room rules in LEGAL.md apply, don't paste vendor SDK knowledge through a model.

## Licensing

Code is licensed under the [Apache License 2.0](LICENSE). Documentation and format specifications are licensed under [Creative Commons Attribution 4.0 International](LICENSE-docs). Contributions use the same terms.

SolidWorks, Rhino, CATIA, Autodesk Fusion, Creo, NX, Parasolid, and other product names are trademarks of their respective owners. cadmpeg uses them only to identify the file formats its decoders target. cadmpeg is an independent project and is not affiliated with, endorsed by, or sponsored by any CAD vendor. See [LEGAL.md](LEGAL.md).
