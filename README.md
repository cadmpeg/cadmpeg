# cadmpeg

A CLI to read, inspect, convert, and write native CAD files. Built on top of libraries that allow other applications to add support for these formats.

Decoders map vendor files into one documented IR: preview meshes, B-rep geometry, design intent, and parametric history where the format carries them. Validators, exporters, and downstream tools share that IR. Progress per format follows an [L0–L9 support ladder](docs/format-support.md#support-ladder).

[Try it](#quick-start) · [Format support](docs/format-support.md) · [Donate a test file](corpus/README.md) · [Contribute](CONTRIBUTING.md)

## Purpose

Native CAD formats are proprietary, sparsely documented and cumbersome to work with.
Building applications that support these formats requires navigating a maze of translator and SDK services without any open source options. cadmpeg aims to offer an open source solution for this problem.

Format knowledge comes from legally possessed CAD files and public documentation. Vendor SDKs, decompiled binaries, and confidential material are prohibited ([LEGAL.md](LEGAL.md)).

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

- **FreeCAD `.FCStd`**: [L5](docs/format-support.md#support-ladder) (schema 4 / file 1)
- **Autodesk Fusion `.f3d`**: [L4](docs/format-support.md#support-ladder)
- **Autodesk Inventor `.ipt`/`.iam`**: [L1](docs/format-support.md#support-ladder)
- **SolidWorks `.sldprt`**: [L4](docs/format-support.md#support-ladder)
- **Rhino `.3dm`**: [L0](docs/format-support.md#support-ladder)
- **Siemens NX `.prt`**: [L2](docs/format-support.md#support-ladder)
- **CATIA V5 `.CATPart`**: [L1](docs/format-support.md#support-ladder)
- **Creo `.prt`**: [L1](docs/format-support.md#support-ladder)
- **STEP Part 21 AP203/AP214/AP242**: [L9](docs/format-support.md#support-ladder)
- **IGES 5.1/5.2/5.3 Fixed ASCII**: [L8](docs/format-support.md#support-ladder)
- **ASM/ACIS `.sat`/`.smt`/`.smb`/`.sab` streams**: [L3](docs/format-support.md#support-ladder) (admitted binary and text branches)

[Format support](docs/format-support.md) holds profiles and scoring rules. [`docs/formats/`](docs/formats/) holds byte semantics and open items.

## Pipeline

```text
input file ──▶ container decoder ──▶ format decoder ──▶ IR ──▶ validator ──▶ exporter ──▶ output + reports
```

- [CAD IR version 5](docs/cad-ir.md)
- [Architecture](docs/architecture.md)
- [Format support](docs/format-support.md)
- [Roadmap](docs/roadmap.md)

## CLI

Convert a native file to another format:

```sh
cadmpeg convert part.f3d -f step -o part.step
```

Inspect a native file:

```sh
cadmpeg inspect part.sldprt
```

```text
format: sldprt (detected high)
container: sldprt-blocks
entries: 58
...
notes:
  - active Parasolid B-rep candidate: Contents/Config-0-Partition
```

## Contributing

Public test files are the current need. If you can dedicate a CAD file to the public domain under CC0, [donate it to the corpus](corpus/README.md).

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

AI-assisted contributions are welcome when reviewed and concise. Clean-room rules apply: do not pass vendor SDK knowledge through a model.

## Licensing

Code uses the [Apache License 2.0](LICENSE); documentation and specifications use [CC BY 4.0](LICENSE-docs). Contributions use the corresponding license.

SolidWorks, Rhino, CATIA, Autodesk Fusion, Autodesk Inventor, Creo, NX, Parasolid, ShapeManager, ACIS, and other product names are trademarks of their respective owners. cadmpeg uses them only to identify the file formats its decoders target. cadmpeg is an independent project and is not affiliated with, endorsed by, or sponsored by any CAD vendor. See [LEGAL.md](LEGAL.md).
