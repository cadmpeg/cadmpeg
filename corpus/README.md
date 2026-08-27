# cadmpeg corpus

This directory accepts contributor-authored CAD fixtures dedicated to the public domain under CC0-1.0. CAD files enter it only through the donation process below. Donation rules follow the clean-room policy in [LEGAL.md](../LEGAL.md).

## What we accept

A donated file must meet all of these requirements:

1. **You authored it.** You created the file yourself in the CAD application you are declaring.
2. **You modeled every included part.** Start from a blank or default template. Every part, material, and appearance in the file is your own work.
3. **Your CAD license permits this use.** Author under license terms that allow public sharing of outputs. Check educational and trial EULAs before donating; many restrict use of outputs.
4. **You dedicate it CC0-1.0.** You release the file into the public domain via [Creative Commons CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). This lets anyone use it as a decoder test fixture without restriction.
5. **Notes name what it exercises.** State the surface type, feature, assembly structure, or edge case under test.
6. **It has a manifest entry** (see below).

## How to donate

1. Author a file you are happy to place in the public domain.
2. Compute its SHA-256:
   ```sh
   shasum -a 256 my_part.f3d      # macOS
   sha256sum my_part.f3d          # Linux
   ```
3. Add an entry to the corpus manifest describing it (format below), using [`manifest.example.toml`](manifest.example.toml) as a template.
4. Open a pull request (or an issue, if the file is large and you need guidance on how to attach it) with the file and its manifest entry. Confirm explicitly in the PR/issue text: the CC0 dedication, that every included part is your own work, and that your CAD license permits sharing it.

The first accepted donation creates `corpus/manifest.toml`. Each later donation adds a `[[file]]` entry. Manifest and donation verification is manual until verification tooling lands. Maintainers verify the manifest fields and declarations before merge.

## Manifest format

Valid format keys are `f3d`, `fcstd`, `inventor`, `sldprt`, `catia`, `nx`, `creo`, `rhino`, `iges`, `step`, and `sat`. The manifest records each file's name, format key, authoring application and version, source URL, acquisition date, CC0 dedication, SHA-256, purpose, and optional expected topology. See [`manifest.example.toml`](manifest.example.toml) for an annotated template. A minimal entry:

```toml
[[file]]
filename = "bracket_single_body.f3d"
format = "f3d"                       # f3d | fcstd | inventor | sldprt | catia | nx | creo | rhino | iges | step | sat
authoring_app = "Autodesk Fusion 360"
authoring_app_version = "2.0.19426"
source_url = "https://github.com/example/cadmpeg-corpus/pull/1"
acquisition_date = "2026-07-14"
license = "CC0-1.0"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
notes = "Minimal single prismatic body; exercises container and planar faces."

# Optional: expected topology, so decoders can assert against it.
[file.expected_topology]
bodies = 1
faces = 6
edges = 12
vertices = 8
```

`expected_topology` is optional. Include only counts reported by the authoring application.

## The derived `dialect` field

One manifest field is not donor-supplied. `dialect` holds the `docs/dialects.toml` id that the codec's own `inspect()` reads out of the file's bytes, pinned like a golden. Do not write it by hand and do not copy it from the registry: run

```sh
UPDATE_CORPUS_DIALECTS=1 cargo test -p cadmpeg --test corpus_manifest
```

and review the diff. `cargo test -p cadmpeg --test corpus_manifest` then compares the pins against a fresh classification, so a codec change that reclassifies a corpus file is a test failure. The test verifies each `sha256` first: a pin describes the exact bytes it was derived from.

`dialect` and `authoring_app_version` are different axes. The first is what the bytes are, the second is what wrote them. Neither is derivable from the other, so the manifest carries both.
