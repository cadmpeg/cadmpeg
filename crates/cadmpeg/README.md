# cadmpeg

`cadmpeg` inspects native CAD containers, decodes supported model data into
CADIR, validates and compares CADIR models, and exports CADIR or STEP AP214.
It also writes supported `.FCStd`, `.f3d`, and `.sldprt` models.

Native codecs transfer different subsets of geometry, topology, design intent,
presentation, and metadata. Check [format support][support] before relying on a
conversion path.

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

Or install from crates.io:

```sh
cargo install cadmpeg
```

## Convert a model

`convert` decodes the input, validates the resulting CADIR, then exports it:

```sh
cadmpeg convert bracket.f3d -o bracket.step
```

The output extension selects `step`, `fcstd`, `f3d`, `sldprt`, or `cadir`. Pass
`--format` when writing to standard output or when the filename does not
identify the format:

```sh
cadmpeg convert bracket.f3d --format step > bracket.step
```

Conversion stops before export if validation finds errors. It also refuses
geometry output when decoding transfers no geometry. `--allow-invalid` and
`--allow-empty` override these checks. These flags permit output; they do not
repair or add model data.

## Inspect, decode, and validate

Inspect a native container without decoding its model:

```sh
cadmpeg inspect bracket.f3d
cadmpeg inspect bracket.f3d --json
```

Decode a native file to canonical CADIR JSON:

```sh
cadmpeg decode bracket.f3d -o bracket.cadir.json
```

Without `--output`, artifact-producing commands write only the artifact to
standard output. Diagnostics and loss summaries go to standard error, so
redirection remains safe.

Validate either CADIR or a supported native file:

```sh
cadmpeg validate bracket.cadir.json
cadmpeg validate bracket.f3d --json
```

Use `export` to skip CADIR validation:

```sh
cadmpeg export bracket.cadir.json -o bracket.step
```

`--container-only` stops native decoding before geometry. It is useful for
container diagnostics and normally requires `--allow-empty` when followed by a
geometry export.

## Read raw bytes

`cadmpeg inspect` also carries byte tools that work on any file, decoded or not.
They replace the ad-hoc `xxd`, `od`, `strings`, and `struct.unpack` one-liners
that format work otherwise needs. Every offset and length argument accepts `0x`
hexadecimal or decimal, and `_` between digits.

```sh
cadmpeg inspect hex part.prt --offset 0x40 --len 0x80   # dump with an ASCII gutter
cadmpeg inspect read part.prt --type u32 --offset 0x40  # one scalar, decimal and hex
cadmpeg inspect read part.prt --type f64 --offset 0x100 --count 8 --stride 24
cadmpeg inspect find part.prt --hex '4d5a??00'          # `??` is a byte wildcard
cadmpeg inspect find part.prt --utf16le Extrude
cadmpeg inspect strings part.prt --min 6 --encoding both
cadmpeg inspect struct part.prt --offset 0x100 --count 4 \
  --layout 'u32le:id,pad4,f64le:x,f64le:y,f64le:z'
cadmpeg inspect container part.f3d                      # ZIP entries with byte offsets
cadmpeg inspect diff probe-a.prt probe-b.prt            # positional byte diff
```

`--le` and `--be` select the byte order for `read`; little-endian is the
default. `read --count N` walks a record array, stepping `--stride` bytes and
defaulting to the scalar width.

Common alternative spellings are accepted: `--length` for `--len`, `--min-len`
and `--min-length` for `--min`, `--start` for `--offset`, and `-n` for
`--count`. `cadmpeg inspect bytes <tool>` runs the same tool as `cadmpeg inspect
<tool>`. `find` needs its pattern on `--hex`, `--ascii`, or `--utf16le`, because
a bare word does not say how to encode it; the tool stops at `--max` hits and
says so, and `--max 0` reports every hit.

`--layout` is a comma-separated record spec with no implicit alignment:

| Field | Meaning |
| --- | --- |
| `u8`, `i8` | One byte. A byte-order suffix is rejected. |
| `u16le`, `i32be`, `f64le`, … | A wide scalar. The `le` or `be` suffix is required. |
| `bytesN` | `N` raw bytes, printed as hexadecimal. |
| `padN` | `N` bytes skipped and not printed. Takes no name. |

Every field except `padN` accepts an optional `:name`; unnamed fields are called
`f<index>`. `struct --count N` decodes `N` consecutive records and reports an
error rather than a partial record when the run passes end of file.

`inspect container` lists ZIP entries with their local-header and payload
offsets, so a hex dump of an entry follows directly. Names are single-quoted
because Fusion `.f3d` entry names hold `[` and `]`, which a shell reads as a
glob. Other container families are listed by `cadmpeg inspect FILE` through
their codec.

`inspect diff` compares byte `n` of one file with byte `n` of the other. It
reports the first differing offset, the differing byte count, and the differing
spans. `--gap` merges two spans separated by that many equal bytes or fewer.
This is a byte comparison; `cadmpeg diff` compares decoded models instead.

A file whose name is also a subcommand name, such as `hex`, is read as the
subcommand. Write `./hex` for such a file.

## Compare models

`diff` compares two decoded models structurally:

```sh
cadmpeg diff before.cadir.json after.cadir.json
cadmpeg diff before.f3d after.f3d --json
```

It reports unit, tolerance, arena membership, and modified entity fields.
Exit status `1` indicates a difference.

## Inputs and outputs

The built-in codecs recognize `.f3d`, `.FCStd`, `.sldprt`, `.3dm`, `.CATPart`,
IGES, STEP, and the NX and Creo `.prt` layouts by content. Commands that load
models also accept CADIR JSON. Use `--input-format` to bypass detection for an
ambiguous or extensionless input.

Output formats are:

- `cadir` for canonical CADIR JSON; `json` is an alias.
- `step` for ISO 10303-21 STEP AP214.
- `fcstd`, `f3d`, `rhino`, and `sldprt` for the native writers' supported subsets.

Native writing may depend on retained source data and rejects unsupported
edits. The [format support page][support] defines each reader and writer's
current semantic coverage.

File output is atomic. cadmpeg refuses to replace its input or an existing
output unless `--force` is present. An explicit `--format` takes precedence
over a conflicting output extension and emits a warning.

## Losses and machine-readable reports

Native decoding prints whether geometry transferred and lists known losses.
STEP export reports omitted, reduced, or normalized content. To save a
versioned JSON record of a `decode`, `export`, or `convert` operation, pass
`--report`:

```sh
cadmpeg convert bracket.f3d -o bracket.step \
  --report bracket.conversion.json
```

The command report contains decode, validation, and export sections when those
stages ran. `inspect --json`, `validate --json`, and `diff --json` write
versioned JSON directly to standard output.

`inspect`, `validate`, and `diff` produce no artifact beside the report, so they
also accept `-o` and `--output` for the report path. Every command refuses to
replace an existing report or artifact unless `--force` is present. Validation
findings live under `.validation_report.findings`; the printed error count and
exit status 1 count the same `error` and `blocking` findings.

## Exit status

- `0`: the requested operation completed; validation passed and diffs were empty
  when those checks ran.
- `1`: semantic result, such as validation errors, a non-empty diff, or refusal
  to export invalid or empty geometry.
- `2`: operational error, including invalid arguments, unrecognized input,
  decode or encode failure, and file-system errors.

Run `cadmpeg help <command>` for the complete options of a command.

## More documentation

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
