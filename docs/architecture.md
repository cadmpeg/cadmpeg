# cadmpeg architecture

cadmpeg routes native CAD through format codecs into `CadIr` version 5. Source fidelity rides beside the product document as a sidecar. Codecs may validate and encode afterward. [cad-ir.md](cad-ir.md) defines the IR. Crate docs and `cadmpeg --help` define APIs and CLI options. [dialects.toml](dialects.toml) owns dialect identity, [dialect-support.toml](dialect-support.toml) owns per-dialect capability, and [format-support.md](format-support.md) renders aggregate format capability.

## Pipeline

```text
native CAD ── detect + inspect ──> container summary

native CAD or CADIR ── load + decode/parse ──> CadIr
                                                ├── dump ──> .cadir.json
                                                ├── check ──> check report
                                                └── convert ── check + plan + write ──> output artifact
```

- `convert` loads or decodes, checks, then writes another format. `--allow-errors` writes after check errors.
- `inspect` detects a codec and reports container structure without decoding geometry.
- `diff` reads or decodes two inputs and compares units, tolerances, the neutral model, native namespaces, source metadata, source annotations, and retained records. ID-bearing records match by globally unique IDs. Vector position is not entity identity. A source attribute whose key ends in `_local_sha256` holds a machine-local content digest, which no two platforms reproduce; `diff` reports a difference in one under a separate informational section and keeps status 0.
- `check` reads or decodes an input and checks IR invariants. Decoder and export admission subsets are in [admissibility-routes.md](admissibility-routes.md).
- `dump` runs the selected codec and serializes `CadIr` as CADIR JSON.

CADIR input parses directly into `CadIr`. The parser accepts exactly IR version 5, including its required `subds` arena. Source annotations and retained records stay in the source-fidelity sidecar. `--allow-empty` permits geometry export when a source decode transferred no geometry.

A successful dump is not a checked model.

## Decode session

The safe consumer trait is `Codec` (`inspect` / `decode`). Format crates implement the raw hook trait `CodecBackend` (`inspect_impl` / `decode_impl`). The `Codec` blanket wrapper acquires the root input under `DecodePolicy` limits, records the container-only request, runs the backend, and finalizes a `DecodeContext`. Its strict gate evaluates full-decode reports only: a container-only report keeps its losses and is never refused by that gate.

`DecodeContext` holds budget counters and the address-space registry. `DecodeArena` holds byte buffers with stable addresses. A `Copy` `View` carries bounded, space-tagged navigation. `DecodeOptions` carries a `policy` field. Ownership lives in `cadmpeg_core::decode`. Classify a named `MAX_*` cap with [decode-resource-caps.md](decode-resource-caps.md) before adding a bound or a `ResourceLimits` field.

Semantic decode is bounded by the caller's `DecodePolicy`. The policy limits input bytes, temporary materialization, retained bytes, admitted entities, collection items, recursive depth, and algorithm work. File-declared counts, pointer walks, recursive definitions, and geometric recovery cannot override these limits. A refused request returns a structured resource refusal and does not return a partial semantic document. The policy is implementation admission control; no source field changes it.

## CLI stream and exit contract

`dump` and `convert` reserve stdout for the output artifact. Diagnostics use stderr. `--report <path>` writes a machine-readable command report with `schema_version: 7`, which always emits the dialect fields: `dialects` on every container summary and decode report, `target` on every export report, and `dialect` on every source metadata block. A classified source dialect owns its declarations inside its `DialectMatch`; source metadata has no second declaration field. Version 6 added top-level `status` (`ok` | `refused`) and `refusal` (`{ stage, code, message }` or null), including semantic refusal paths. A codec-level decode failure during `dump` with an explicit report is a `decode`-stage `decode_failed` refusal; an I/O failure remains an operational exit. JSON from `inspect`, `check`, and `diff` uses the same CLI schema version. That envelope version is independent of `CadIr.ir_version`.

Status 0 is success. Status 1 is a semantic refusal, including strict decode policy. Status 2 is an operational failure. A strict refusal from `dump` or `convert` uses `refusal.code: strict_decode_rejected` and serializes its completed decode report. A codec failure from either command uses status 2 and `refusal.code: decode_failed` when an explicit command report is written.

Writers create a unique temporary file in the destination directory, then rename it into place. `--force` replaces an existing file. The CLI rejects an output path that resolves to the input.

## Loss reports

Source decoders return `DecodeReport`, including a typed `DecodeTransfer`, a decode-coverage census, notes, and attributable `LossNote` entries. `DecodeTransfer` distinguishes container-only from full decode and records the geometry outcome only for a full decode. The JSON form retains `container_only` and `geometry_transferred`; both cannot be true. Validation propagates supplied decode losses unchanged.

Each codec owns a `*LossCode` enum in `src/loss.rs`. Every reported drop goes through `code.note(message)`, which pins the namespaced local code, `LossTaxonomy`, severity, and strict floor. Shared taxonomy is the category used for subsystem reporting; the stable machine-readable identifier is the codec-local `family.detail` string.

Every encoder returns an `ExportReport` with its format id, entity census, loss notes, informational notes, and a `write_path`. STEP reports reductions and omitted IR data. CADIR export carries no losses. IGES, F3D, and SLDPRT select among preservation and semantic generation according to their own delivery laws. FreeCAD patches a retained document. Rhino and STEP synthesize from neutral IR. Unsupported delivery is refused atomically. Decode losses remain in the command report when convert started from native CAD.

`write_path` names which of an encoder's write paths produced the bytes: `verbatim_replay` copies retained source bytes out unchanged, `patched` runs the writer over retained source content, and `synthesized` runs the writer over neutral IR content alone. The encoder sets it at the branch it takes. The distinction is not recoverable from the output, because a patch that changes nothing observable reproduces its input byte for byte. F3D and SLDPRT take all three paths. IGES takes `verbatim_replay` and `synthesized`. FreeCAD is always `patched`. CADIR, STEP, and Rhino are always `synthesized`.

Export-side refusal is not owned by the `Encoder` trait. The conversion layer owns `--reject-lossy`: the application transcoder refuses to plan when the decode report carries any loss and refuses to write when the planned `ExportReport` carries any loss. Both are policy stops distinct from a planning failure, and neither consults per-loss strict floors — any loss note refuses. The STEP writer always emits the representable subset and records reductions in its report; `--reject-lossy` after `plan` is the remaining owner of refusal for those losses. Other writers can reject unsupported input during planning. A format specification's "strict export rejects" sentence names the owning writer policy where one exists and the conversion stop otherwise.

## Invariants

These hold across every codec, every dialect, and every release. A change that breaks one is a change to the architecture, not to a codec.

**Every classified report has one primary layer.** `ContainerSummary` and `DecodeReport` store classified dialects as `DialectLayers`, whose constructor requires one primary layer. Extra layers are unique by `(format, instance)`. An extra layer in the primary format requires an instance, so it cannot masquerade as a second primary. `None` means the report is unclassified. The invariant is enforced at construction; report consumers read the primary through `DialectLayers::primary` and do not search a flat list or panic to repair an invalid shape. Both report types carry typed `LossNote` values. Inspection charges every loss it resolves directly instead of encoding loss facts as informational strings.

**Per-entity persistent identity.** Every entity carries a globally unique id under the entity-ID grammar, and that id is what `diff`, `query graph`, `query join`, and golden stability are built on. A dialect never appears in an id: classification refines, and an id that moved when the classifier improved would churn every diff and every golden against no collision that has ever been constructed.

**The neutral model is dialect-free.** The neutral arenas and model records hold no dialect discriminant and do not branch on one. `CadIr.source` carries `SourceMeta.dialect`, the primary source `DialectMatch`, so classification survives a CADIR intermediate. Container, decode, and export reports carry the corresponding source layers or output target. `cadmpeg-core` carries the dialect types and no format-version branches.

**cadmpeg's own version axes are separate.** `CadIr.ir_version`, `NativeNamespace::version`, report `schema_version`, and `DECODE_SIDECAR_VERSION` describe cadmpeg. A source dialect describes a file someone else wrote. Different lifecycles, different owners, different failure modes; they never share a type and no operation compares one to the other.

**Retention and the three write paths are unconditional.** `SourceFidelity`, retained records, and the `verbatim_replay` / `patched` / `synthesized` distinction are how cadmpeg writes back what it does not understand. Retention is never gated on a version check, and no codec is split per dialect: one codec owns its dialect set and branches inside itself. Splitting either way removes the cross-version upgrade path.

**Decoders tolerate unknown records generationally.** A record a decoder does not recognize is retained, not fatal, and not a reason to select a different decoder. This is the open-world read architecture — the same rule sfnt and protobuf converged on. Version-selected decoder variants are not a cleanup of it; they are its replacement, and they break the previous invariant.

**Transfer accounting is explicit.** Every drop goes through a `*LossCode` with a pinned `family.detail` string, a `LossTaxonomy`, a severity, and a strict floor. IGES also fills `TransferLedger` so its read, write, and loss dispositions can be checked for closure. Other codecs currently leave that ledger empty. The dialect id in a report joins a claim to one file's classification.

**Write reports are honest about bytes.** `ExportReport.target` names the dialect actually produced, on every write path, including the inherited dialect under preservation. The claim is verified against the output: re-decoding the written bytes through the codec's own classifier lands on exactly the dialect the report named. A report that names a target the bytes do not classify as is a defect, not a rounding.

**Fidelity resolution describes sidecar consumption only.** `NotProvided` means no source-fidelity sidecar was offered, `NotConsumed` means the writer did not consume an offered sidecar, `Replayed` means preserved source content was consumed, and `Degraded` means offered fidelity was eligible but could not be consumed. A same-format write that selects a target other than the source dialect charges `target.source-dialect-displaced` through the codec loss vocabulary. Dialect displacement does not make fidelity degraded.

**Refusal is structural, never a version allowlist.** A genuinely undecodable variant is refused. A decodable document is never refused because its version is not on a list: the residual `unknown` row parses through its selected recovery route and charges the dialect-unverified loss. `Admission::AdmittedUnverified.using` names a declared grammar only when that grammar was actually substituted; a direct residual route leaves it absent. Strict mode plus that loss is the only refusal-policy axis; a second one would drift against the first. Enforcement: every `read = "refused"` row in `docs/dialect-support.toml` carries a typed `refusal` cause. The closed vocabulary admits only a recognized alternate encoding with no parser grammar or evidence that selects no framing grammar. A discriminant value alone ("save format not in the band", "archive word outside the declared set") is never a reason; a row that offers one is a defect in the codec, not a fact about the format, and the fix is the selected recovery attempt with `Admission::AdmittedUnverified` and the charged loss. Symmetry with an existing gate is not a structural reason either — an unjustified refusal is not made sound by building a second one to match it.

**The registries are two files and never one.** `docs/dialects.toml` states which dialects exist. It describes formats, so it changes when a vendor ships. `docs/dialect-support.toml` states what cadmpeg does with each. It describes cadmpeg, so it changes per commit. Merging them makes cadmpeg's coverage look like a property of the format, which is the claim that must never become writable.

## Crate map

| Crate                    | Responsibility                                                                                                                         |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `cadmpeg`                | CLI orchestration, conversion workflow, native validators, and output dispatch.                                                        |
| `cadmpeg-registry`       | Codec registration, prefix detection, `identify`, the output-format vocabulary, and the embedded dialect registries.                   |
| `cadmpeg-ir`             | `CadIr` version 5, validation, diff, codec traits, reports, and source-fidelity sidecars.                                              |
| `cadmpeg-core`           | Shared decode budgets, arenas, views, container summaries, and I/O helpers.                                                            |
| `cadmpeg-container`      | Shared archive and compression helpers for container codecs.                                                                           |
| `cadmpeg-parasolid`      | Shared Parasolid schema-token grammar and embedded-layer identity.                                                                      |
| `cadmpeg-protein`        | Shared schema and paged instance-property decoding for Protein asset packages.                                                         |
| `cadmpeg-codec-freecad`  | FreeCAD `.FCStd` inspection, decode, retained-document patching, and repacking.                                                         |
| `cadmpeg-codec-f3d`      | Fusion `.f3d` inspection, ASM/SAB geometry, design records, retained replay, selected native edits, and semantic generation.           |
| `cadmpeg-codec-inventor` | Inventor `.ipt`/`.iam` compound, RSe, part geometry, external occurrences, properties, appearances, and design-record decode.          |
| `cadmpeg-codec-sldprt`   | SolidWorks `.sldprt` container, Parasolid B-rep, features, retained replay, retained-record patching, and semantic generation.          |
| `cadmpeg-codec-rhino`    | Rhino `.3dm` inspection, decode, and semantic writing.                                                                                  |
| `cadmpeg-codec-catia`    | CATIA V5 `.CATPart` layout inspection, carrier decode, and neutral projection.                                                         |
| `cadmpeg-codec-nx`       | NX `.prt` `SPLMSSTR` extraction, Parasolid carriers, and neutral projection.                                                           |
| `cadmpeg-codec-creo`     | Creo `.prt` section decode, placed geometry, and neutral projection.                                                                   |
| `cadmpeg-codec-sat`      | Bare ASM/ACIS `.sat`/`.smt`/`.smb`/`.sab` stream inspection and B-rep transfer outside any container.                                  |
| `cadmpeg-codec-iges`     | IGES Fixed ASCII, Compressed ASCII, and Binary decode; retained replay; and bounded Fixed ASCII semantic writing.                       |
| `cadmpeg-codec-step`     | STEP Part 21 inspection, decode, semantic writing, and export loss accounting.                                                         |
| `cadmpeg-fuzz`           | Nightly `cargo-fuzz` targets outside the default workspace.                                                                            |

## Codec interface

Each input codec implements `Codec`:

- `id() -> &'static str` names the codec for registry lookup and `--input-format`.
- `detect(&[u8]) -> Confidence` identifies a format from a byte prefix.
- `inspect(&mut dyn ReadSeek) -> Result<ContainerSummary, CodecError>` enumerates container structure and reports the losses resolved at inspection depth.
- `decode(&mut dyn ReadSeek, &DecodeOptions) -> Result<DecodeResult, DecodeFailure>` produces `CadIr`, `DecodeReport`, and source fidelity. `DecodeFailure::Codec` carries backend and resource failures.

`--input-format` selects a codec. Without it, the CLI detects one. Native writers use the separate `Encoder` trait. The Rust trait definitions are authoritative for exact signatures.

Strict decode refuses at the `Codec::decode` wrapper, which returns `DecodeFailure::StrictRejected` for the first reported loss whose strict consequence is `Reject`. The wrapper applies that predicate to full-decode reports only: a container-only decode keeps its losses and is admitted in either mode. The refusal carries the loss code, loss message, and completed `DecodeReport`. It is not a `CodecError`: a strict refusal reports a policy decision after decode, not a defect in the bytes. A codec reports its losses with their strict floors and adds no strict gate of its own. A local gate widens the refusal predicate and reclassifies the refusal where the caller cannot see it.
