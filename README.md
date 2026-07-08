# cadmpeg

**Open tooling for reading native CAD files.**

cadmpeg decodes native CAD files into an intermediate representation (IR), validates that IR, and exports neutral formats with explicit loss reporting. The project is an _ffmpeg for CAD_ with byte-level decode inspection.

```
cadmpeg inspect   part.f3d             # container-level streams and segments
cadmpeg decode    part.f3d -o part.cadir.json   # native bytes -> inspectable IR
cadmpeg validate  part.cadir.json      # is the IR internally consistent?
cadmpeg export    part.cadir.json -f step -o part.step   # export without validation
cadmpeg convert   part.f3d -f step -o part.step           # decode, validate, export
cadmpeg diff      a.cadir.json b.cadir.json         # structural changes; exit 1 when different
```

Example with a Fusion 360 `.f3d` file:

```
$ cadmpeg convert part.f3d -f step -o part.step
decode report (f3d): geometry_transferred=true
losses:
  [info/geometry] 22 spline surface record(s) were decoded into NURBS carriers.
  ...
validation: OK (0 error(s), 0 warning(s))
wrote part.step (2125 entities)
```

cadmpeg exports the decoded analytic and NURBS B-rep to STEP. Each stage of **inspect → decode → validate → export** reports its coverage.

`--input-format` bypasses automatic source-format detection on single-input commands. `export` does not validate its input; `convert` runs decode, validation, and export, and refuses invalid IR unless `--allow-invalid` is passed. Geometry exports from native decodes that transferred no geometry are refused unless `--allow-empty` is passed. Output formats are `cadir` (`json` remains an alias) and `step`. When `-f` is omitted, `export` and `convert` infer the format from `-o`; `.cadir`, `.json`, `.step`, and `.stp` are recognized. Stdout output requires `-f`. Existing output and report files require `--force`.

### CLI contract

Artifact-producing commands (`decode`, `export`, and `convert`) write only the artifact to stdout. Diagnostics, decode and validation reports, losses, and output-path notices go to stderr. Their `--report <path>` option atomically writes a pretty JSON report with `schema_version: 1`, the command name, decode and validation reports where applicable, and export counts and losses. The report is written on semantic refusal paths and uses `--force` for overwrite permission. Report commands (`inspect`, `validate`, and `diff`) write their report to stdout; `--json` wraps their machine-readable result with `schema_version: 1` and the command name. Exit status 0 means success, 1 means a semantic failure (validation failure, refused empty geometry, or a structural difference), and 2 means an operational error such as I/O, decoding, report writing, or invalid flags. Diff compares units, tolerances, and every IR arena by entity ID, including modified entity fields. Entity IDs are positional and deterministic within one decode, so comparing unrelated source files can produce a noisy result.

---

## Why cadmpeg

Native CAD formats (`.sldprt`, `.f3d`, `.CATPart`, Creo `.prt`, NX `.prt`) are private binary formats owned by CAD vendors. They store model data in terms of vendor geometry kernels and application-specific feature histories, with little or no public specification. STEP export gives users an interchange file, but not the original design history or a format they can inspect and maintain independently. cadmpeg starts from the opposite constraint: the bytes in the native file should be recoverable, inspectable, and usable without access to the original CAD application.

cadmpeg uses four design constraints:

- **Byte traceability.** Decoders emit only byte-derived values. IR values record source offsets where available, and `Exactness` provenance marks derived or inferred fields.
- **Loss accounting.** Each export emits a report of what the exporter carried, approximated, or dropped.
- **Clean-room inputs.** Specs come from CAD files we are legally allowed to possess, without vendor SDKs, decompiled binaries, or confidential material. See [LEGAL.md](LEGAL.md).

---

## Current status

**cadmpeg is early.** `.f3d` → IR → STEP is implemented. The support matrix lists the current rung for each path.

- **`.f3d` decode (in-repo):** the container and active ASM/SAB stream decode into an exact B-rep with analytic and NURBS carriers, inline/ref pcurves, body transforms, and linked source attributes. Subtype-table references resolve through tag-aware spans. Nested Protein assets join through Design body maps, MetaStream object ids, and ACT channels into typed appearance assets and body bindings.
- **STEP export (in-repo):** a pure-Rust ISO 10303-21 (AP214) writer emits manifold B-rep solids with analytic and B-spline carriers, and reports everything it could not represent.
- **`.CATPart` decode (in-repo):** `inspect` detects and names all five storage variants; for the standard-nested variant the vertex cloud and curved analytic surfaces decode as unattached carriers. STEP export emits no geometry until topology reconstruction lands.
- **NX `.prt` decode (in-repo):** the SPLMSSTR container and embedded Parasolid streams decode; analytic surfaces and curves decode as carriers, value-validated against paired STEP exports, and where the stream's topology records resolve the B-rep graph is reconstructed and attached. Streams that yield no topology are reported as a counted loss.
- **Creo `.prt` decode (in-repo):** magic-based detection (it shares an extension with NX), section enumeration, and PSB token decoding work. `geometry_transferred=false` because the format stores prototype geometry that cadmpeg cannot present as placed model geometry. The loss report names seven gates.
- **`.sldprt` decode (in-repo):** the CRC-validated container and the active Parasolid partition decode into analytic B-rep solids that survive the same IR → STEP path. Open gates: bodies are not yet separated into shells, B-spline faces stay opaque, and periodic seams are missing. Each gate appears in the loss report.
- All five formats also have research specs in [`docs/formats/`](docs/formats/). Those specs document research coverage and the open gates behind each in-repo gap.
- **The corpus starts empty.** Every decoder above was validated against files we cannot redistribute. If you can author a CAD file and dedicate it CC0, [donating it to the corpus](corpus/README.md) gives the project public test coverage.

For the per-format fidelity breakdown, see [docs/format-support.md](docs/format-support.md). Planned work is tracked in [docs/roadmap.md](docs/roadmap.md).

The support matrix is authoritative when it conflicts with this README. Please open an issue for stale README claims.

---

## How it works

```
input file ──▶ container decoder ──▶ format decoder ──▶ IR ──▶ validator ──▶ exporter ──▶ output + loss report
```

The IR connects the pipeline: decoders produce it, validators check it, and exporters consume it. The IR serializes to JSON for direct decode inspection. For the IR's shape and guarantees, see [docs/cad-ir.md](docs/cad-ir.md). For the pipeline and crate map, see [docs/architecture.md](docs/architecture.md).

---

## Repository layout

```
cadmpeg/
├── crates/                  # Rust workspace (built in parallel; see docs/architecture.md)
│   ├── cadmpeg/             # CLI: inspect / decode / validate / export / diff (+ convert sugar)
│   ├── cadmpeg-ir/          # the intermediate representation
│   ├── cadmpeg-codec-f3d/   # .f3d codec (analytic B-rep + inline cached NURBS)
│   ├── cadmpeg-codec-sldprt/# .sldprt codec (analytic B-rep; body/shell split is the open gap)
│   ├── cadmpeg-codec-catia/ # .CATPart codec (variant detect + carriers; topology is the open gap)
│   ├── cadmpeg-codec-nx/    # NX .prt codec (container + analytic carriers; topology is the open gap)
│   ├── cadmpeg-codec-creo/  # Creo .prt codec (container + PSB structure; geometry gated on format unknowns)
│   ├── cadmpeg-step/        # pure-Rust STEP (AP214) writer
│   └── cadmpeg-fuzz/        # fuzz targets
├── docs/
│   ├── architecture.md      # pipeline, crate map, decode philosophy
│   ├── cad-ir.md            # the IR spec
│   ├── format-support.md    # the L0–L6 fidelity matrix
│   ├── roadmap.md           # phased plan + good first issues
│   └── formats/             # per-format research specs (f3d, sldprt, catia, creo_prt, siemens_nx)
├── corpus/                  # donation pipeline for openly-licensed test files (no binaries at launch)
├── LICENSE                  # Apache-2.0 (code)
├── LICENSE-docs             # CC-BY-4.0 (docs & specs)
├── LEGAL.md                 # clean-room posture, forbidden sources, takedown process
└── CONTRIBUTING.md          # DCO sign-off, provenance declaration, how to add a codec
```

---

## Building

cadmpeg is a standard Cargo workspace. From the repository root:

```sh
cargo build --workspace
cargo test  --workspace
```

STEP export is pure Rust and needs no extra dependencies.

---

## Contributing

Two kinds of contributions unblock the decoders: **codec implementations** (turn a documented format spec into a decoder) and **format research** (close a spec's open gates with byte-backed evidence). Validators, exporters, IR tooling, and corpus tooling also need work.

Two hard requirements before you send code:

1. Every commit carries a **DCO sign-off** (`git commit -s`).
2. Decoder and spec contributions additionally carry a **provenance declaration** attesting how the knowledge was obtained.

Read [CONTRIBUTING.md](CONTRIBUTING.md) for the full process and [LEGAL.md](LEGAL.md) for what sources are and are not acceptable. Good first issues are listed in [docs/roadmap.md](docs/roadmap.md).

---

## Licensing

cadmpeg uses a split license:

- **Code** is licensed under the **Apache License 2.0** (see [LICENSE](LICENSE)).
- **Documentation and format specifications** are licensed under **Creative Commons Attribution 4.0 International (CC-BY-4.0)** (see [LICENSE-docs](LICENSE-docs)).

We accept contributions under the same terms; see [CONTRIBUTING.md](CONTRIBUTING.md).

---

SolidWorks, CATIA, Fusion 360, Creo, NX, Parasolid, and other product names are trademarks of their respective owners. cadmpeg uses them nominatively, only to identify which file formats a decoder targets. cadmpeg is an independent project, not affiliated with, endorsed by, or sponsored by any CAD vendor. See [LEGAL.md](LEGAL.md).
