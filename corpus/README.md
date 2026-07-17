# cadmpeg corpus

This directory accepts contributor-authored CAD fixtures dedicated to the public domain under CC0-1.0. CAD files enter it only through the donation process below.

---

## What we accept

A donated file must meet all of these requirements:

1. **You authored it.** You created the file yourself in the CAD application you are declaring. Do not donate files made by others, customer parts, or anything you found, even if it seems freely available.
2. **It contains no vendor library content.** Start from a blank or default template and model the geometry yourself. A file you authored can still embed vendor-copyrighted material: content-library parts, toolbox/standard components, vendor-supplied materials or appearance assets. Files that pull in such content are not accepted.
3. **Your CAD license permits this use.** The file must be authored under license terms that allow sharing it this way. Files from educational or trial licenses whose EULA restricts use of outputs (education-watermarked files, non-commercial-only terms) are not accepted.
4. **You dedicate it CC0-1.0.** You release the file into the public domain via [Creative Commons CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). This lets anyone use it as a decoder test fixture without restriction.
5. **It has a purpose.** It exercises something: a surface type, a feature, an assembly structure, an edge case a decoder should handle. A note on what it is meant to test helps review.
6. **It is accompanied by a manifest entry** (see below).

We do **not** accept vendor sample files, files under any non-CC0 terms, or files whose origin you are unsure of. This mirrors the clean-room rules in [LEGAL.md](../LEGAL.md): we reject any file with questionable provenance.

---

## How to donate

1. Author a file you are happy to place in the public domain.
2. Compute its SHA-256:
   ```sh
   shasum -a 256 my_part.f3d      # macOS
   sha256sum my_part.f3d          # Linux
   ```
3. Add an entry to the corpus manifest describing it (format below), using [`manifest.example.toml`](manifest.example.toml) as a template.
4. Open a pull request (or an issue, if the file is large and you need guidance on how to attach it) with the file and its manifest entry. Confirm explicitly in the PR/issue text: the CC0 dedication, that the file contains no vendor library content, and that your CAD license permits sharing it.

The first accepted donation creates `corpus/manifest.toml`. Each later donation adds a `[[file]]` entry. Manifest and donation verification is manual until verification tooling lands. Maintainers verify the SHA-256, format key, manifest fields, authorship declaration, CAD-license permission, absence of vendor library content, and CC0 dedication before merge.

---

## Manifest format

Valid format keys are `f3d`, `fcstd`, `sldprt`, `catia`, `nx`, `creo`, `rhino`, and `iges`. The manifest records each file's name, format key, authoring application and version, source URL, acquisition date, CC0 dedication, SHA-256, purpose, and optional expected topology. See [`manifest.example.toml`](manifest.example.toml) for an annotated template. A minimal entry:

```toml
[[file]]
filename = "bracket_single_body.f3d"
format = "f3d"                       # f3d | fcstd | sldprt | catia | nx | creo | rhino | iges
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

`expected_topology` is optional. Include only counts reported by the authoring application; do not guess.
