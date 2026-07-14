# IGES implementation checklist

## Repository boundary

- [x] Every committed format rule is stated in repository terminology and testable without analysis-only inputs.
- [x] Original fixtures are authored directly from the format specification.
- [x] No analysis-only path, identity, content, transformed artifact, or evidence statement enters source, tests, seeds, documentation, reports, or commit messages.
- [ ] Public fixtures have independently verified redistribution terms and provenance in the corpus manifest.

## Phase 0 approvals

- [ ] A maintainer approves `corpus/iges-envelope-a.toml` as the closed IGES 5.3 Fixed ASCII mechanical/document profile.
- [ ] A maintainer approves the L0, L3, L4, L6, and L7 decisions in `docs/format-support.md`.
- [ ] A maintainer approves the generic byte-ledger schema, validation, canonical order, diff behavior, serialization, and IR-version policy.
- [ ] Each support gate has original-fixture and public-fixture classes plus machine-checkable assertions.

## Implementation invariants

- [x] Detection reads a bounded prefix and assigns high confidence only to valid Fixed ASCII framing.
- [x] Inspection does not construct geometry.
- [x] Card, Global, Directory Entry, Parameter Data, graph, and projection layers remain independently testable.
- [ ] Checked arithmetic and configured limits cover counts, offsets, allocation sizes, Hollerith lengths, graph depth, transform depth, retained bytes, and derived tessellation.
- [ ] Malformed input returns deterministic errors or findings and never panics.
- [x] Projection does not reparse source bytes.
- [x] Topology candidates validate before attachment.
- [ ] Score changes and their cumulative assertions land in the same commit.

## Release closure

- [ ] The generated matrix report contains no admitted branch without a decoder, destination, original fixture, public fixture, and assertion.
- [ ] Every repository fixture has zero byte-ledger gaps and overlaps.
- [ ] Repeated decode produces byte-identical canonical output and reports on supported CI platforms.
- [ ] Formatting, build, tests, documentation tests, clippy, audit, and fuzz smoke pass.
- [ ] A final repository scan finds no analysis-only material or dependency.
