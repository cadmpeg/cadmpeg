# cadmpeg roadmap

This roadmap tracks work after the initial `.f3d` → IR → STEP implementation. Status language matches [format-support.md](format-support.md).

Supported format paths use the same sequence: **inspect → decode → validate → export**, with loss accounting throughout.

---

## Implemented

- Rust workspace and CLI: `inspect`, `decode`, `validate`, `export`, `diff`, and `convert`.
- `cadmpeg-ir`: topology arenas, geometry carriers, exactness labels, byte provenance, validation, and JSON Schema generation.
- Pure-Rust STEP AP214 export for manifold B-rep solids with explicit loss reports.
- In-repo codecs for `.f3d`, `.sldprt`, `.CATPart`, NX `.prt`, and Creo `.prt`; see [format-support.md](format-support.md) for each codec's current rung.
- Governance and legal: README, LICENSE (Apache-2.0), LICENSE-docs (CC-BY-4.0), LEGAL.md, CONTRIBUTING.md, CI, and the corpus donation process.
- Format research specs under [`formats/`](formats/).

---

## Current focus

Add public test coverage and implement the decode work required for supported exports.

- Extend generated-byte coverage for decoded native envelopes without adding corpus fixtures.
- Split `.sldprt` bodies into shells and recover periodic seam edges.
- Attach topology for `.CATPart` and NX `.prt` where the byte-level specs identify carrier records.
- Keep each codec's loss report aligned with its actual decode envelope.

---

## Next export targets

`cadmpeg-step` implements STEP export.

- Define additional export targets when their implementations are ready.

---

## External claim threshold

External claims should wait until `cadmpeg convert part.f3d -f step -o part.step` passes on public CC0 corpus files, writes STEP, and emits loss reports. Broader format claims should wait until the support matrix lists the corresponding in-repo capability.

---

## Good first issues

Tasks that do not require format-reversing expertise. See [CONTRIBUTING.md](../CONTRIBUTING.md) for the process (DCO sign-off always; provenance declaration for decoder/spec work).

- **IR JSON schema tooling**: publish the JSON Schema that `cadmpeg-ir` already generates for the `*.cadir.json` IR, and build a validator that checks IR files against it.
- **Validators**: add specific IR consistency checks (e.g. every coedge references a live edge; loops close; face-loop orientation).
- **GLB exporter**: implement a mesh export path and its loss report.
- **Corpus manifest tooling**: a script that verifies manifest SHA-256 values against files, checks required fields, and lints CC0 declarations.
- **Hex-annotation tooling**: a viewer/formatter that renders a decode's byte provenance over a hex dump, to make `inspect` output auditable at a glance.
- **Spec gap research**: pick an open gate in a [`formats/`](formats/) spec and document byte-backed evidence and results. See CONTRIBUTING.md's "contribute format research" path.
