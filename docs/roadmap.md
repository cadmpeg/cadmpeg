# cadmpeg roadmap

cadmpeg aims to read CAD documents into typed IR, write native files, and convert between formats with named losses. [Format support](format-support.md) states the current capabilities and their evidence criteria. The [dialect registry](../crates/cadmpeg-registry/docs/dialect-support.toml) declares each format's read and write envelope.

## Current foundation

CADIR has a committed [JSON Schema](../crates/cadmpeg-ir/schema/cadir.schema.json). `cadmpeg check` includes face-orientation findings from measurement; [measurement.md](measurement.md) defines those findings. The public [corpus manifest](../corpus/manifest.toml) records specimens, and the `corpus_manifest` integration tests verify them.

## Priorities

- Expand geometry, topology, design, product, and presentation coverage within each declared format envelope. Record unresolved format questions in the corresponding open-items document.
- Extend semantic writing and translation while preserving source meaning and reporting every loss.
- Improve semantic identity across round trips and file revisions, and compare geometry within declared tolerances.
- Harden parsers, validators, and writers against malformed input and resource exhaustion. Keep output deterministic.
- Extend independent interoperability checks and public fixtures for supported read and write paths.

## Formats

The registry covers Alibre, ASM/ACIS, CATIA V5, Creo Parametric, Creo View, eDrawings, FreeCAD, Fusion, IGES, Inventor, IronCAD, NX, Onshape, OpenUSD, Rhino, Solid Edge, SolidWorks, and STEP. Each format's profile in [format-support.md](format-support.md) states its current scope and gaps.

## Contributor entry points

- Donate focused fixtures through the [corpus process](../corpus/README.md).
- Resolve a bounded item in a format's `*-open-items.md` file.
- Extend validation, measurement, semantic writing, or interoperability checks for a declared format envelope.
- Improve IR schema tooling, exporters, corpus tooling, inspection, CLI behavior, or documentation.

See [CONTRIBUTING.md](../CONTRIBUTING.md) for contribution requirements.
