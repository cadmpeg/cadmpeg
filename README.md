# cadmpeg

Inspect and convert native CAD files from the command line. Add native CAD import to an application.

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

Inspect a file:

```sh
cadmpeg inspect part.sldprt
```

Convert a file:

```sh
cadmpeg convert part.f3d -o part.step
```

```text
decode report (f3d): depth=geometry, geometry_transferred=true
...
check: OK (0 error(s), 0 warning(s))
wrote part.step (... entities)
```

Measure one face, or every cylindrical face of a part:

```sh
cadmpeg measure part.step --face face#46
cadmpeg measure part.step --faces --kind cylinder --tsv
```

Compare the geometry of two files, for example a native file and its STEP export:

```sh
cadmpeg diff part.f3d part.step --geometry
```

[`docs/measurement.md`](docs/measurement.md) states what each value means and how exact it is.

## Format support

Each format carries one support level from the ladder; `cadmpeg inspect <file>` answers for a specific file.

<!-- generated: capability-lines -->

- **Autodesk Inventor `.ipt` and `.iam`** — L3 ([profile](docs/format-support.md#autodesk-inventor-ipt-and-iam))
- **ASM/ACIS bare `.sat`/`.smt`/`.smb`/`.sab` streams** — L3 ([profile](docs/format-support.md#asmacis-bare-satsmtsmbsab-streams))
- **FreeCAD `.FCStd`** — L5 ([profile](docs/format-support.md#freecad-fcstd))
- **IGES** — L9 ([profile](docs/format-support.md#iges))
- **Rhino `.3dm`** — L2 ([profile](docs/format-support.md#rhino-3dm))
- **SolidWorks `.sldprt`** — L3 ([profile](docs/format-support.md#solidworks-sldprt))
- **Fusion 360 `.f3d`** — L5 ([profile](docs/format-support.md#fusion-360-f3d))
- **Siemens NX `.prt`** — L1 ([profile](docs/format-support.md#siemens-nx-prt))
- **CATIA V5 `.CATPart`** — L3 ([profile](docs/format-support.md#catia-v5-catpart))
- **Creo Parametric `.prt`** — L1 ([profile](docs/format-support.md#creo-parametric-prt))
- **Creo View `.pvz`** — L3 ([profile](docs/format-support.md#creo-view-pvz))
- **OpenUSD layers** — L2 ([profile](docs/format-support.md#openusd-layers))
- **STEP Part 21** — L8 ([profile](docs/format-support.md#step-part-21))

<!-- /generated: capability-lines -->

[Format support](docs/format-support.md) holds profiles and scoring rules. [`docs/formats/`](docs/formats/) holds byte semantics and open items.

## Pipeline

```text
input file ──▶ container decoder ──▶ format decoder ──▶ IR ──▶ validator ──▶ exporter ──▶ output + reports
```

- [CAD IR version 8](docs/cad-ir.md)
- [Architecture](docs/architecture.md)
- [Format support](docs/format-support.md)
- [Roadmap](docs/roadmap.md)

## Contributing

Public test files are the current need. If you can dedicate a CAD file to the public domain under CC0, [donate it to the corpus](corpus/README.md).

Other contributions:

- Implement a codec from a format specification.
- Resolve an open format item with byte-backed evidence.
- Add validators, exporters, IR tooling, corpus tooling, or CLI improvements.

Pull requests require a DCO sign-off; decoder and specification changes also require a provenance declaration. See [CONTRIBUTING.md](CONTRIBUTING.md), [LEGAL.md](LEGAL.md), and the [roadmap](docs/roadmap.md).

## Development

From the repository root:

```sh
cargo build --workspace
cargo test-fast
```

Run an end-to-end smoke test:

```sh
cargo run -p cadmpeg-ir --example emit_cube > cube.cadir.json
cadmpeg convert cube.cadir.json -f step -o cube.step
```

AI-assisted contributions are welcome when reviewed and concise. Clean-room rules apply: do not pass vendor SDK knowledge through a model.

## Licensing

Code uses the [Apache License 2.0](LICENSE); documentation and specifications use [CC BY 4.0](LICENSE-docs). Contributions use the corresponding license.

SolidWorks, Rhino, CATIA, Autodesk Fusion, Autodesk Inventor, Creo, NX, Parasolid, ShapeManager, ACIS, and other product names are trademarks of their respective owners. cadmpeg uses them only to identify the file formats its decoders target. cadmpeg is an independent project and is not affiliated with, endorsed by, or sponsored by any CAD vendor. See [LEGAL.md](LEGAL.md).
