# cadmpeg

**One open pipeline for native CAD.**

`cadmpeg` is the command-line front end for inspecting, decoding, validating,
comparing, and converting CAD files through a documented, provenance-aware
intermediate representation.

> cadmpeg is early software. Every native format has partial support. Commands
> report unsupported or reduced content instead of silently claiming a complete
> conversion.

## Install

Prebuilt binaries are available through Homebrew and the release installers:

```sh
brew install cadmpeg/tap/cadmpeg
```

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/cadmpeg/cadmpeg/releases/latest/download/cadmpeg-installer.sh | sh
```

You can also install the Rust package:

```sh
cargo install cadmpeg
```

cadmpeg requires Rust 1.88 or later.

## Use

Convert a Fusion 360 part to STEP:

```sh
cadmpeg convert part.f3d -f step -o part.step
```

Inspect or decode without exporting:

```sh
cadmpeg inspect part.f3d
cadmpeg decode part.f3d -o part.cadir.json
cadmpeg validate part.cadir.json
```

The command report records validation findings and any source or export content
that could not be represented.

## Formats

The built-in codecs cover Fusion 360 `.f3d`, SolidWorks `.sldprt`, CATIA V5
`.CATPart`, Siemens NX `.prt`, and Creo Parametric `.prt`. Export targets are
CADIR, STEP AP214, and supported SolidWorks `.sldprt` content.

See the [format support profiles][support] for the current capability matrix.

## Project links

- [Repository and full project README][repo]
- [Format support][support]
- [Architecture][architecture]
- [Contributing][contributing]
- [Clean-room and legal policy][legal]

Code is licensed under the Apache License 2.0. cadmpeg is independent of and is
not endorsed by any CAD vendor.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[contributing]: https://github.com/cadmpeg/cadmpeg/blob/main/CONTRIBUTING.md
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md
