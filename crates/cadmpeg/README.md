# cadmpeg

`cadmpeg` inspects native CAD containers, dumps supported model data into
CADIR, checks and compares CADIR models, and exports CADIR, STEP Part 21
(AP203, AP214, or AP242), and bounded IGES 5.1, 5.2, or 5.3 Fixed ASCII. It also writes
supported `.FCStd`, `.f3d`, `.sldprt`, and `.3dm` models.

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

`convert` decodes the input, checks the resulting CADIR, then exports it:

```sh
cadmpeg convert bracket.f3d -o bracket.step
cadmpeg convert bracket.f3d -o bracket.ap242.step --step-target ap242e3
cadmpeg convert bracket.f3d -o bracket.step --reject-step-losses
cadmpeg convert bracket.f3d -o bracket.igs --iges-target 5.3
```

`--step-target` selects the STEP application protocol and edition
(`ap203e1`, `ap203e2`, `ap214` default, `ap242e1`, `ap242e2`, `ap242e3`).
`--reject-step-losses` refuses STEP output before writing when any STEP loss
note would be reported.

`--iges-target 5.1`, `5.2`, or `5.3` selects the IGES target. The default is
`5.3`.

The output extension selects `step`, `iges`, `fcstd`, `f3d`, `sldprt`, `rhino`, or
`cadir`. Pass `--format` (alias `--to`) when the filename does not identify
the format, or when a text format (`cadir`, `step`) goes to standard output:

```sh
cadmpeg convert bracket.f3d --format step > bracket.step
```

Binary formats (`fcstd`, `f3d`, `sldprt`, `rhino`) are refused on standard
output, because that spelling is nearly always `--input-format` (alias
`--from`) written as `--format`; pass `-o FILE` or, for a deliberate binary
pipe, `--binary-stdout`.

Conversion stops before export if the check finds errors. It also refuses
geometry output when decoding transfers no geometry. `--allow-errors` and
`--allow-empty` write the current result anyway. They leave model data
unchanged.

## Inspect, dump, and check

Inspect a native container without decoding its model:

```sh
cadmpeg inspect bracket.f3d
cadmpeg inspect bracket.f3d --json
```

Dump a native file to canonical CADIR JSON:

```sh
cadmpeg dump bracket.f3d -o bracket.cadir.json
```

Without `--output`, artifact-producing commands write only the artifact to
standard output. Diagnostics and loss summaries go to standard error, so
redirection remains safe.

Check either CADIR or a supported native file:

```sh
cadmpeg check bracket.cadir.json
cadmpeg check bracket.f3d --json
```

`--container-only` stops native decoding before geometry. It is useful for
container diagnostics and normally requires `--allow-empty` when followed by a
geometry export.

## Read raw bytes

`cadmpeg inspect` also carries byte tools that work on any file, decoded or
not. Every offset and length argument accepts `0x` hexadecimal or decimal, and
`_` between digits.

```sh
cadmpeg inspect hex part.prt --offset 0x40 --len 0x80   # dump with an ASCII gutter
cadmpeg inspect read part.prt --type u32 --offset 0x40  # one scalar, decimal and hex
cadmpeg inspect read part.prt --type f64 --offset 0x100 --count 8 --stride 24
cadmpeg inspect find part.prt --hex '4d5a??00'          # `??` is a byte wildcard
cadmpeg inspect find part.prt --utf16le Extrude
cadmpeg inspect strings part.prt --min 6 --encoding both
cadmpeg inspect struct part.prt --offset 0x100 --count 4 \
  --layout 'u32le:id,pad4,f64le:x,f64le:y,f64le:z'
cadmpeg inspect container part.f3d                      # ZIP or CFB members
cadmpeg inspect extract part.f3d 'Design/Streams.dat' -o streams.dat
cadmpeg inspect cmp probe-a.prt probe-b.prt             # positional byte compare
```

`--le` and `--be` select the byte order for `read`; little-endian is the
default. `read --count N` walks a record array, stepping `--stride` bytes and
defaulting to the scalar width.

Common alternative spellings are accepted: `--length` for `--len`, `--min-len`
and `--min-length` for `--min`, `--start` for `--offset`, `--step` for
`--stride`, `-n` for `--count`, and `--input FILE` for the positional file on
every single-input tool. `cadmpeg inspect bytes <tool>` runs the same tool as
`cadmpeg inspect <tool>`. `find` needs its pattern on `--hex`, `--ascii`, or
`--utf16le`, because a bare word does not say how to encode it; a guessed
`--type` on `find`, or a text or hex value on `read --type`, gets an error that
names the right flag or tool. `find` stops at `--max` hits and says so,
`--max 0` reports every hit, and `--context N` dumps `N` bytes around each
hit.

`--layout` is a comma-separated record spec with no implicit alignment:

| Field                        | Meaning                                             |
| ---------------------------- | --------------------------------------------------- |
| `u8`, `i8`                   | One byte. A byte-order suffix is rejected.          |
| `u16le`, `i32be`, `f64le`, … | A wide scalar. The `le` or `be` suffix is required. |
| `bytesN`                     | `N` raw bytes, printed as hexadecimal.              |
| `padN`                       | `N` bytes skipped and not printed. Takes no name.   |

Every field except `padN` accepts an optional `:name`; unnamed fields are called
`f<index>`. `struct --count N` decodes `N` consecutive records and reports an
error rather than a partial record when the run passes end of file.

`inspect container` lists ZIP or CFB members. ZIP columns are
header/data/packed/unpacked/method/crc32/name, so a hex dump of an entry
follows directly. CFB columns are id/kind/size/alloc/path; size and alloc
are empty for storages. `--json` prints the same listing as versioned JSON
with raw names and `container_kind` `zip` or `cfb`. Names in the table are
single-quoted because Fusion `.f3d` entry names hold `[` and `]`, which a
shell reads as a glob. `inspect extract FILE MEMBER` writes one ZIP
entry's decompressed, CRC-checked bytes, or one CFB stream, to `-o FILE`
or standard output; the member name matches byte-exactly, brackets
included, where `unzip` reads it as a glob and fails. Other container
families are listed by `cadmpeg inspect FILE` through their codec.

`inspect cmp` compares byte `n` of one file with byte `n` of the other. It
reports the first differing offset, the differing byte count, and the differing
spans. `--gap` merges two spans separated by that many equal bytes or fewer.
It exits 1 if the files are not identical, including length-only (Unix cmp
contract). `cadmpeg diff` compares decoded models.

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

The built-in codecs recognize `.f3d`, `.FCStd`, `.ipt`, `.iam`, `.sldprt`,
`.3dm`, `.CATPart`, IGES, STEP, bare ASM/ACIS `.sat`/`.smt`/`.smb`/`.sab`
streams, and the NX and Creo `.prt` layouts by content. Commands that load
models also accept CADIR JSON. Use `--input-format` (alias `--from`) to bypass
detection for an ambiguous or extensionless input. Every command that takes
one input file also accepts `--input FILE` as a spelling of the positional;
the two-file commands (`diff`, `inspect cmp`, `inspect extract`) take their
inputs positionally only.

Output formats are:

- `cadir` for canonical CADIR JSON; `json` is an alias.
- `step` for ISO 10303-21 Part 21; `--step-target` selects AP203 edition 1 or 2,
  AP214, or AP242 edition 1, 2, or 3.
- `fcstd`, `f3d`, `rhino`, and `sldprt` for the native writers' supported subsets.

Native writers use retained source data where the format requires it, and reject
unsupported edits. The [format support page][support] defines each reader and
writer's current semantic coverage.

File output is atomic. cadmpeg refuses to replace its input or an existing
output unless `--force` is present. An explicit `--format` takes precedence
over a conflicting output extension and emits a warning.

## Losses and machine-readable reports

Native decoding prints whether geometry transferred and lists known losses.
STEP export reports omitted, reduced, or normalized content. To save a
versioned JSON record of a `dump` or `convert` operation, pass
`--report`:

```sh
cadmpeg convert bracket.f3d -o bracket.step \
  --report bracket.conversion.json
```

The command report contains decode, check, and export sections when those
stages ran. `inspect --json`, `inspect container --json`, `check --json`,
and `diff --json` write versioned JSON directly to standard output.

`inspect`, `check`, and `diff` produce no artifact beside the report, so they
also accept `-o` and `--output` for the report path. Every command refuses to
replace an existing report or artifact unless `--force` is present. Check
findings live under `.check_report.findings`; `cadmpeg check` prints the
error count and exits 1 on the same `error` and `blocking` findings.

## Query reports

`query` projects one named view from a JSON artifact without `jq`: it reads a
command report, a decoded CADIR document, or a `<stem>.fidelity.json` decode
sidecar, detects which one it was given, and prints the view. Aggregate views
print tab-separated rows with a header; `item` prints pretty-printed JSON
records.

```sh
cadmpeg check bracket.f3d -o report.json
cadmpeg query findings report.json     # severity  check  entity  message
cadmpeg query losses report.json       # severity  code   message
cadmpeg query coverage report.json     # decode coverage counts
cadmpeg query counts bracket.cadir.json  # per-arena entity counts; alias: arenas
cadmpeg query item bracket.cadir.json model.faces FACE_ID  # one record; alias: record
cadmpeg query summary report.json      # artifact kind and section counts
cadmpeg query schema model.features    # the arena's record type (no FILE)
cadmpeg query fidelity part.cadir.fidelity.json  # retained source records
cadmpeg query fidelity part.cadir.fidelity.json --stream S -o s.bin  # extract
```

`counts` on a CADIR document lists arena lengths for `model` and every
`native.<codec>` namespace; on a check report it lists `entity_counts`.
`item` uses the same dotted arena names as `query counts --json`
(`model.<arena>` or `native.<codec>.<arena>`; a bare name means
`model.<arena>`). It matches the JSON-string `id` field exactly or as a unique
suffix, accepts several IDs in one call, and with no ID prints the first
record (`--head N` for the first N). Follow `links` and run `item` again to
join. `--fields a,b.c` projects those paths as TSV (projection only — no
`--where`). `schema` is the one view that takes no file: it prints the IR's
compile-time record type for a model arena — every field, whether it is
required, and every variant of a tagged union — or, bare, every model arena
and its element type (`sidecar` prints the decode-sidecar shape). Which
arenas a document actually has still comes from `counts`; native arena
records are codec-owned, so `schema` refuses them and names `item` instead.
`fidelity` lists a decode sidecar's retained source records (the extraction
address space) and, with `--stream NAME`, reassembles that stream's retained
bytes byte-exactly into `-o FILE` — refusing gapped extents and extent-only
retention loudly rather than splicing or writing empty output.
An empty or not-run section is not an error: the header prints, a
note goes to standard error, and the exit status stays `0`. A view the
artifact kind can never carry exits `2` and names the command that produces
the right artifact. `--json` wraps the projection in the versioned envelope,
and `-` reads standard input.

## Exit status

Verdict commands exit 1 on a negative verdict. Other commands do not use 1.
Exit 2 is operational on every command.

- `0`: the requested operation completed; a verdict command had a positive
  verdict when it ran.
- `1`: negative verdict. Verdict commands are `convert` (refused write),
  `check` (failed check), `diff` (models differ), and `inspect cmp` (files
  differ). `inspect cmp` exits 1 if the files are not identical, including
  length-only (Unix cmp contract).
- `2`: operational error, including invalid arguments, unrecognized input,
  decode or encode failure, and file-system errors.

Non-verdict commands are `dump`, `query`, and `inspect` (except `inspect cmp`).

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
