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

```sh
cadmpeg convert part.f3d -f step
```

Conversion reports check results and loss:

```text
decode report (f3d): geometry_transferred=true
losses:
  [info/geometry] 22 spline surface record(s) were decoded into NURBS carriers.
  ...
check: OK (0 error(s), 0 warning(s))
wrote part.step (2125 entities)
```

## Format support

Depth is the highest read level any declared dialect of the format reaches;
breadth counts its witnessed dialects at read `L1` or higher. Both come from
`docs/dialects.toml` and `docs/dialect-support.toml`. Run `cadmpeg dialects`
for the per-dialect rows.

<!-- generated: capability-lines -->

- **FreeCAD `.FCStd`**: depth L5, breadth 1 of >=1 ([profile](docs/format-support.md#freecad-fcstd))
- **Autodesk Fusion `.f3d`**: depth L4, breadth 1 of >=1 ([profile](docs/format-support.md#fusion-360-f3d))
- **Autodesk Inventor `.ipt`/`.iam`**: depth none, breadth n/a ([profile](docs/format-support.md#autodesk-inventor-ipt-and-iam))
- **SolidWorks `.sldprt`**: depth none, breadth n/a ([profile](docs/format-support.md#solidworks-sldprt))
- **Rhino `.3dm`**: depth L1, breadth 6 of 8 ([profile](docs/format-support.md#rhino-3dm))
- **Siemens NX `.prt`**: depth none, breadth 0 of >=1 ([profile](docs/format-support.md#siemens-nx-prt))
- **CATIA V5 `.CATPart`**: depth L1, breadth 6 of >=6 ([profile](docs/format-support.md#catia-v5-catpart))
- **Creo Parametric `.prt`**: depth L1, breadth 2 of >=2 ([profile](docs/format-support.md#creo-parametric-prt))
- **STEP Part 21**: depth L9, breadth 4 of >=4 ([profile](docs/format-support.md#step-part-21))
- **IGES**: depth L9, breadth 1 of 21 ([profile](docs/format-support.md#iges))
- **ASM/ACIS bare streams**: depth none, breadth n/a ([profile](docs/format-support.md#asmacis-bare-satsmtsmbsab-streams))

<!-- /generated: capability-lines -->

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
cadmpeg convert cube.cadir.json -f step -o cube.step
```

AI-assisted contributions are welcome when reviewed and concise. Clean-room rules apply: do not pass vendor SDK knowledge through a model.

## Licensing

Code uses the [Apache License 2.0](LICENSE); documentation and specifications use [CC BY 4.0](LICENSE-docs). Contributions use the corresponding license.

SolidWorks, Rhino, CATIA, Autodesk Fusion, Autodesk Inventor, Creo, NX, Parasolid, ShapeManager, ACIS, and other product names are trademarks of their respective owners. cadmpeg uses them only to identify the file formats its decoders target. cadmpeg is an independent project and is not affiliated with, endorsed by, or sponsored by any CAD vendor. See [LEGAL.md](LEGAL.md).
