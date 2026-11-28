# Solid Edge issue work

This guide is for one of issues #1863 through #1874, the pieces of the Solid Edge
`.par`, `.psm` and `.asm` reader tracked in
[#1862](https://github.com/cadmpeg/cadmpeg/issues/1862). Each sub-issue has
Known, Unknown and How to verify sections and an attached CC0 repro set: a
before/after native pair and STEP exports. The attached files are enough to
work the issue; no Solid Edge licence is needed. Follow [AGENTS.md](../../AGENTS.md)
for code, writing and test rules and [CONTRIBUTING.md](../../CONTRIBUTING.md)
for the checks and the pull request.

## Starting point

The reader crate is `crates/cadmpeg-codec-solidedge`. Until the skeleton pull
request [#1875](https://github.com/cadmpeg/cadmpeg/pull/1875) merges, start
from its branch:

```sh
git fetch https://github.com/cadmpeg/cadmpeg pull/1875/head
git switch -c solidedge-issue-1863 FETCH_HEAD
```

Settled format facts are in `docs/formats/solidedge.md`, fixed offsets in
`docs/layouts/solidedge.toml` and open questions in
`docs/formats/solidedge-open-items.md`. The issue's Known section summarizes the
facts that bear on it.

## Inspect the repro files

Download the attachments into a scratch directory outside the repository, then
list and extract the streams with the cadmpeg CLI (`cargo build -q -p cadmpeg`):

```sh
cadmpeg inspect container before.par
cadmpeg inspect extract before.par "$stream" -o before.stream
cadmpeg inspect extract after.par "$stream" -o after.stream
cadmpeg inspect cmp before.stream after.stream
```

Copy each stream path byte for byte from the listing; some names start with a
control character. Read the changed spans with `cadmpeg inspect hex`, `find`,
`strings`, `read` and `struct`. State a hypothesis with record boundary, tag,
width, byte order, count, reference scope and unit, and check it against every
attached file.

`inspect extract` does not overwrite an existing output file; use a new name or
delete the old one first. Save noise changes several streams on every save. A
second save of the base file with no edit shows which spans are noise; compare
each variant against both saves.

## Verify against STEP

After implementing the reading, decode and check both native files and their
STEP exports, and compare them:

```sh
cadmpeg dump after.par -o after.native.json
cadmpeg dump after.step -o after.step.json
cadmpeg check after.par
cadmpeg query counts after.native.json
cadmpeg query counts after.step.json
cadmpeg measure after.native.json
cadmpeg measure after.step.json
cadmpeg diff after.native.json after.step.json --geometry
```

Body and face counts, volume and bounding box must agree within the
allowances in [docs/measurement.md](../measurement.md), and the issue's How to
verify procedure must pass. Tests are synthetic, built from the specification.
There is no shared builder for a compound file with named streams; extend the
crate's `test_support.rs`, starting from `cadmpeg_test_support::compound`.

The layout generator emits only the constants the crate reads, and its test
compiles the crate. For a new record in `docs/layouts/solidedge.toml`, write its
module in `src/layout.rs` by hand in the generated form, then regenerate. A new
production `MAX_*` constant needs a row, the totals and the crate count in
`docs/decode-resource-caps.md`.

## When to ask

Ask on the issue when a needed repro variant is missing, when two layouts fit
the evidence, or when the change needs a CADIR wire change. To request a new
pair, state the exact before/after edit, units and STEP exports needed. A donor
with a licensed Solid Edge install (Community Edition counts) can build it from
a blank default template and attach it under CC0-1.0, following
[corpus/README.md](../../corpus/README.md). The standard recipe set comes from
the public [Solid Edge Fixture Builder](https://fixtures.dutchmancad.xyz/builders/solidedge/).
For issues #1863 and #1865 through #1874, the
[Solid Edge repro generator](https://fixtures.dutchmancad.xyz/builders/solidedge/repro/)
builds the issue's before/after pair: `build_repro.ps1 -Issue <n>` writes a new
folder per run, removes identity data, scans the output and reports PASS or
FAIL. Attach only files from a run that reports PASS.
