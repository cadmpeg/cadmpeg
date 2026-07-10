# cadmpeg

`cadmpeg` is a command-line tool for inspecting native CAD files, converting
them to STEP or CADIR, comparing models, and validating model structure.

Native format support is still growing. The [format support page][support]
lists what works for each file type.

## Install

Install with Homebrew:

```sh
brew install cadmpeg/tap/cadmpeg
```

Or use the installer for macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/cadmpeg/cadmpeg/releases/latest/download/cadmpeg-installer.sh | sh
```

You can also build it with Cargo:

```sh
cargo install cadmpeg
```

## Use

Convert an `.f3d` file to STEP:

```sh
cadmpeg convert part.f3d -f step -o part.step
```

Inspect or decode without exporting:

```sh
cadmpeg inspect part.f3d
cadmpeg decode part.f3d -o part.cadir.json
cadmpeg validate part.cadir.json
```

## Formats

The built-in codecs recognize `.f3d`, `.sldprt`, `.CATPart`, and the NX and
Creo `.prt` layouts. cadmpeg writes CADIR and STEP AP214, and can update parts
of retained `.f3d` and `.sldprt` files.

## Documentation

- [Format support][support]
- [Architecture][architecture]
- [Contributing][contributing]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[contributing]: https://github.com/cadmpeg/cadmpeg/blob/main/CONTRIBUTING.md
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md
