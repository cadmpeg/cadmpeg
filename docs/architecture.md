# cadmpeg architecture

cadmpeg routes native CAD through format codecs into `CadIr` version 8. Source fidelity rides beside the product document as a sidecar. Codecs may validate and encode afterward. [cad-ir.md](cad-ir.md) defines the IR. Crate docs and `cadmpeg --help` define APIs and CLI options. [dialects.toml](dialects.toml) owns dialect identity, [dialect-support.toml](../crates/cadmpeg-registry/docs/dialect-support.toml) owns per-dialect capability, and [format-support.md](format-support.md) renders aggregate format capability.

## Pipeline

```text
native CAD ── detect + inspect ──> container summary

native CAD or CADIR ── load + decode/parse ──> CadIr
                                                ├── dump ──> .cadir.json
                                                ├── check ──> check report
                                                └── convert ── check + plan + write ──> output artifact
```

- `convert` loads or decodes, checks, plans, then writes another format. It refuses before the write when an error finding is in what the planned payload writes. The plan's `ArenaCoverage` decides this: `Complete` writes every arena and native namespace; `Declared` writes the model arenas marked `Written` or `Reported` and the native namespace of the output format only. A finding is in the payload when its entity or an identity in its message is in it, when it names no entity, or when its entity is not in the document. If planning fails, every error refuses. The report keeps every finding. `--allow-errors` writes after any check error. `check` still exits 1 on every error.
- `inspect` detects a codec and reports container structure without decoding geometry.
- `diff` reads or decodes two inputs and compares scope, units, tolerances, the neutral model, native namespaces, source metadata, source annotations, retained records, and unrepresented classes. ID-bearing records match by globally unique IDs. Vector position is not entity identity. A source attribute whose key ends in `_local_sha256` holds a machine-local content digest, which no two platforms reproduce; `diff` reports a difference in one under a separate informational section and keeps status 0.
- `check` reads or decodes an input and checks IR invariants. Decoder and export admission subsets are in [admissibility-routes.md](admissibility-routes.md).
- `dump` runs the selected codec and serializes `CadIr` as CADIR JSON.

CADIR input parses directly into `CadIr`. The parser accepts exactly IR version 8, including its required `subds` arena. Source annotations and retained records stay in the source-fidelity sidecar. `--allow-empty` permits geometry export when a source decode transferred no geometry.

`convert` requests `geometry` for STEP, IGES, and Rhino and `full` for other encoders through `Encoder::request`. `--full-decode` requests full independently of the target, as does `--reject-lossy`. `--container-only` requests container scope and conflicts with `--full-decode`; its restriction to CADIR output is caller policy. `dump`, `check`, and `diff` request full. Stderr reports delivered content scope, not an ordered decode depth.

A successful dump is not a checked model.

## Source recovery

Default decoding attempts every readable part of the source. A format's
conformance requirement is not, by itself, a reason to refuse the document.
Absent, reordered, or invalid descriptive metadata produces a diagnostic.
Retain nonconforming source content through the existing fidelity path. Do not
invent a replacement author, date, or producer version.

Distinguish descriptive metadata from fields used to interpret bytes or model
values. A recovery must state which grammar, unit, or value it uses. If a field
cannot be interpreted, omit the smallest dependent record or component whose
boundary is known and continue with independent content. Stop when the parser
cannot establish boundaries or identities needed for further decoding, or a
resource limit is reached. Do not convert every parse error into a warning.

Use codec-local diagnostics and loss codes. Keep conformance checks that provide
useful diagnostics, but run them outside the fatal admission path when they do
not control interpretation. No per-format strictness feature is needed. Existing
explicit loss policies remain separate from default recovery.

Header recovery uses this distinction in STEP, IGES, Rhino, and FreeCAD. STEP
reads header records by name and retains nonconforming metadata; its schema
admission remains separate. IGES resolves Global fields individually after it
has read the delimiters. Rhino can start its table scan without a comment.
FreeCAD producer-version metadata does not select the persistence schema.

## Decode session

The safe consumer trait is `Codec` (`inspect` / `decode`). Format crates implement `CodecBackend`. The sealed wrapper acquires bounded input, records the requested `DecodeDepth`, runs the backend, and finalizes the session. The codec sets `CadIr.scope` before any digest; the wrapper validates populated-class invariants and derives `DecodeTransfer.scope` from the document. Missing required classes produce `depth.not-delivered`, naming the classes. A codec can over-deliver. FreeCAD and F3D can omit neutral design projection for a geometry request. FreeCAD retains readable design objects as typed native records; F3D marks design unrepresented because its cut does not produce typed design records. The shared depth harness checks unchanged content outside design and source-fidelity containment. See [document-contract.md](document-contract.md) for the state, event, and patch contracts.

`DecodeContext` holds budget counters and the address-space registry. `DecodeArena` holds byte buffers with stable addresses. A `Copy` `View` carries bounded, space-tagged navigation. `DecodeOptions` carries a `policy` field. Ownership lives in `cadmpeg_core::decode`. Classify a named `MAX_*` cap with [decode-resource-caps.md](decode-resource-caps.md) before adding a bound or a `ResourceLimits` field.

Semantic decode is bounded by the caller's `DecodePolicy`. The session counts input bytes, decompressed bytes, retained bytes, admitted entities, and recursive depth. Retained bytes are counted where decoded output keeps bytes: native record text, arena buffers, and copies of source bytes. A session ceiling refuses: the decode returns a structured resource refusal and no partial semantic document. A file-declared count is validated against the bytes that carry it before it sizes an allocation. A search, iteration, or geometric recovery runs under its own local work bound; when the bound is reached the decoder skips that item and reports the loss, and the session continues. The policy is implementation admission control; no source field changes it.

## CLI stream and exit contract

`dump` and `convert` reserve stdout for the output artifact. Diagnostics use stderr. `--report <path>` writes a machine-readable command report. The report carries the four-state dialect admission wire and always emits the dialect fields: `dialects` on every container summary, decode report, and source metadata block, and `target` on every export report. Each source layer owns its declarations inside its `DialectMatch`; source metadata has no second declaration field. The report carries top-level `status` (`ok` | `refused`) and `refusal` (`{ stage, code, message, dialects?, target?, resource?, unrepresented? }` or null), including semantic refusal paths. `dialects` carries every identified layer on an unsupported-source refusal. `target` carries the typed request state and complete encoder catalog on an unsupported-target refusal. A plan-stage `unrepresented_source` refusal carries `unrepresented: { classes, path }`: retained bytes cannot supply missing typed content to a patch or replay. Synthesized paths do not refuse on scope. `--allow-empty` controls the separate empty-geometry policy. A codec decode failure is a `decode`-stage `decode_failed` refusal. A resource refusal uses `code: resource_limit` and carries `resource: { dimension, reason, limit, used, requested, operation }`. `dimension` is `input_bytes`, `decompressed_bytes`, `retained_bytes`, `entities`, or `recursion_depth`. `reason` is `budget_exceeded` or `allocation_failed`. The integer fields retain the limit in force, the usage recorded by the failed operation, and the additional amount requested. `operation` is the label of that operation and also appears in the diagnostic on stderr. `stage` identifies the decode, check, plan, or export stage that requested the resource. An I/O failure remains an operational exit. `inspect`, `check`, and `diff` write the same report envelope to stdout with `--json` and to the requested `--report` file, including refusal paths. A binary-stdout refusal also writes an explicitly requested report. `query --json` writes that envelope to stdout with `command` `query`, the view name in `view`, and the view result in `payload`. A `command` value is the bare top-level command word; a subcommand identity is a payload field.

Status 0 is success. Status 1 is a semantic refusal, including strict decode policy. Status 2 is an operational failure. A strict refusal from `dump` or `convert` uses `refusal.code: strict_decode_rejected` and serializes its completed decode report. Codec and resource failures use status 2 and preserve their refusal evidence in the requested report. `--max-input-bytes` overrides the input-byte ceiling of the selected `--limits` profile for decode and container inspection.

Writers create a unique temporary file in the destination directory, then rename it into place. `--force` replaces an existing file. The CLI rejects an output path that resolves to the input.

## Loss reports

Source decoders return `DecodeReport`, including `DecodeTransfer { scope, geometry_transferred }`, coverage, notes, and attributable losses. Scope is the document's delivered state; the geometry boolean records whether any B-rep landed, including when model is held but empty. Validation propagates supplied decode losses unchanged.

Each codec owns a `*LossCode` enum in `src/loss.rs`. Every reported drop goes through `code.note(message)`, which pins the namespaced local code, `LossTaxonomy`, severity, and strict floor. Shared taxonomy is the category used for subsystem reporting; the stable machine-readable identifier is the codec-local `family.detail` string.

Every encoder returns an `ExportReport` with target, census, losses, notes, neutral and native coverage, write path, and optional synthesis cause. Unsupported content is reduced or omitted with loss. Only malformed input, format capacity, or target preconditions can refuse. The CLI retains the decode report independently of the export report.

`write_path` records the selected mechanism: `verbatim_replay` copies eligible retained bytes unchanged; `patched` uses retained layout; `synthesized` authors content from the document, including the writer's own typed native namespace. Patch and replay cannot supply content beyond the document. Nonempty `unrepresented` forbids either retained-byte path. A declined patch selects synthesis with `PatchDeclined { reason }`. F3D and SLDPRT support all three paths, IGES replay and synthesis, FreeCAD patch and synthesis, and CADIR, STEP, and Rhino synthesis.

Each encoder declares `EncoderBackend::REQUEST: DecodeDepth`: geometry for STEP, IGES, and Rhino, full for CADIR, F3D, SLDPRT, and FreeCAD. `Encoder::request` exposes it to callers. This is the decode request that normally supplies complete output, not a minimum document scope. Writers synthesize from the content held at any valid scope; excluded classes produce notes. Complete-and-empty arenas are meaningful and produce no omission loss.

The caller owns loss policy. The CLI defaults to writing the representable subset. `--reject-lossy=decode` judges only this invocation's native decode or CADIR parse; a restored sidecar is printed as history. `--reject-lossy=export` judges planned export losses; `any` applies both. `--reject-lossy-floor` defaults to `info` and selects the minimum severity for rejection. A sub-full document scope produces a note under every rejection setting, never a policy refusal.

## Invariants

These hold across every codec, every dialect, and every release. A change that breaks one is a change to the architecture, not to a codec.

**Every classified report has one primary layer.** `ContainerSummary` and `DecodeReport` store classified dialects as `DialectLayers`, whose constructor requires one primary layer. Extra layers are unique by `(format, instance)`. An extra layer in the primary format requires an instance, so it cannot masquerade as a second primary. `None` means the report is unclassified. The invariant is enforced at construction; report consumers read the primary through `DialectLayers::primary` and do not search a flat list or panic to repair an invalid shape. Both report types carry typed `LossNote` values. Inspection charges every loss it resolves directly instead of encoding loss facts as informational strings.

**Per-entity persistent identity.** Every entity carries a globally unique id under the entity-ID grammar, and that id is what `diff`, `query graph`, `query join`, and golden stability are built on. A dialect never appears in an id: classification refines, and an id that moved when the classifier improved would churn every diff and every golden against no collision that has ever been constructed.

**The neutral model is dialect-free.** The neutral arenas and model records hold no dialect discriminant and do not branch on one. `CadIr.source` carries `SourceMeta.dialects`, the complete source `DialectLayers`, so primary and nested or carried classification survive a CADIR intermediate. This is source metadata, not neutral-model semantics. Same-format target inheritance reads only the primary match. Container, decode, and export reports carry the corresponding source layers or output target. `cadmpeg-core` carries the dialect types and no format-version branches.

**cadmpeg's own version axis is separate.** `CadIr.ir_version` describes cadmpeg. It is the single version stamp: the CADIR document, the decode sidecar, and the CLI report are exact-match on it and carry no migrations. A source dialect describes a file someone else wrote. Different lifecycles, different owners, different failure modes; they never share a type and no operation compares one to the other.

**Retention does not supply document content.** Source fidelity remains independent of dialect admission. Typed native records belong to the document; raw retained bytes remain layout data. Writers regenerate neutral/native disagreements in favor of neutral records or drop them with loss. Unknown records cannot survive through replay when their content has no typed representation. One codec owns its dialect set and branches internally.

**Decoders tolerate unknown records generationally.** A record a decoder does not recognize is retained, not fatal, and not a reason to select a different decoder. This is the open-world read architecture — the same rule sfnt and protobuf converged on. Version-selected decoder variants are not a cleanup of it; they are its replacement, and they break the previous invariant.

**Transfer accounting is explicit.** Every drop goes through a `*LossCode` with a pinned `family.detail` string, a `LossTaxonomy`, a severity, and a strict floor. IGES also fills `TransferLedger` so its read, write, and loss dispositions can be checked for closure. Other codecs currently leave that ledger empty. The dialect id in a report joins a claim to one file's classification.

**Write reports are honest about bytes.** `ExportReport.target` names the dialect actually produced, on every write path, including the inherited dialect under preservation. The claim is verified against the output: re-decoding the written bytes through the codec's own classifier lands on exactly the dialect the report named. A report that names a target the bytes do not classify as is a defect, not a rounding.

**Fidelity resolution describes sidecar consumption only.** `NotProvided` means no source-fidelity sidecar was offered, `NotConsumed` means the writer did not consume an offered sidecar, `Replayed` means preserved source content was consumed, and `Degraded` means offered fidelity was eligible but could not be consumed. A same-format write that selects a target other than the source dialect charges `target.source-dialect-displaced` through the codec loss vocabulary. Dialect displacement does not make fidelity degraded.

**Refusal is structural, never a version allowlist.** A genuinely undecodable variant is refused. A decodable document is never refused because its version is not on a list: the residual `unknown` row parses through its selected recovery route and charges the dialect-unverified loss. `Admission::Unverified.using` names the declared grammar that was substituted. `Admission::Residual` marks a direct residual route with no declared grammar. Strict mode plus that loss is the only refusal-policy axis; a second one would drift against the first. Enforcement: every `read = "refused"` row in `crates/cadmpeg-registry/docs/dialect-support.toml` carries a typed `refusal` cause. The closed vocabulary admits only a recognized alternate encoding with no parser grammar or evidence that selects no framing grammar. A discriminant value alone ("save format not in the band", "archive word outside the declared set") is never a reason; a row that offers one is a defect in the codec, not a fact about the format, and the fix is the selected `Unverified` or `Residual` recovery with the charged loss. Symmetry with an existing gate is not a structural reason either — an unjustified refusal is not made sound by building a second one to match it.

**The registries are two files and never one.** `crates/cadmpeg-registry/docs/dialects.toml` states which dialects exist. It describes formats, so it changes when a vendor ships. `crates/cadmpeg-registry/docs/dialect-support.toml` states what cadmpeg does with each. It describes cadmpeg, so it changes per commit. Merging them makes cadmpeg's coverage look like a property of the format, which is the claim that must never become writable.

**Dialect identity has one author.** `crates/cadmpeg-registry/docs/dialects.toml` is the source of every codec dialect-id constant. `scripts/check-dialects.py --write-generated` projects each format into its owning private Rust module; the default check compares those modules byte for byte with the registry-derived output. Codec-local enums map parser facts to those constants. They do not repeat id strings. The generated modules contain constants only, not a runtime table or a shared dialect enum.

## Crate map

| Crate                    | Responsibility                                                                                                                         |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `cadmpeg`                | CLI orchestration, conversion workflow, native validators, and output dispatch.                                                        |
| `cadmpeg-registry`       | Codec registration, prefix detection, `identify`, the output-format vocabulary, and the embedded dialect registries.                   |
| `cadmpeg-ir`             | `CadIr` version 8, validation, diff, codec traits, reports, and source-fidelity sidecars.                                              |
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

Strict decode refuses at the `Codec::decode` wrapper, which returns `DecodeFailure::StrictRejected` for the first reported loss whose strict consequence is `Reject`. The wrapper applies that predicate to geometry and full requests only: a container-only request keeps its losses and is admitted in either mode. The refusal carries the loss code, loss message, and completed `DecodeReport`. It is not a `CodecError`: a strict refusal reports a policy decision after decode, not a defect in the bytes. A codec reports its losses with their strict floors and adds no strict gate of its own. A local gate widens the refusal predicate and reclassifies the refusal where the caller cannot see it.
