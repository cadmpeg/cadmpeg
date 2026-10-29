# The document contract

CADIR is the truth. Decoders report; callers decide; writers write what the document holds.

A decode produces a document and a report. The report describes an event: what the decoder could not read, approximated, or skipped. The document describes a state: what it holds. Nothing in the document records why something is absent. After decode, the document does not defer to its source for anything.

Policy (whether a loss is acceptable) belongs to the caller: the CLI through flags, an editor through its UI. The CLI defaults to decoding and writing as much as possible and reporting losses.

A writer never refuses because of where content came from, or because it guesses the caller does not want the output. A writer may refuse only a precondition: an input from which no valid file of the target format can be produced. Everything else it writes, degraded where necessary, and reports as export loss.

Patching and replay may choose bytes, layout, and identities; they may never supply content. A patched or replayed output must mean what a synthesized output from the same document means. Retained source bytes are a layout optimization, never a content source.

Typed native records are part of the document. A writer's own native namespace contains typed, validated, queryable, diffable records retained according to [native-arena-disposition.md](native-arena-disposition.md). Opaque retained bytes and `unknowns` blobs are not typed content. Neutral arenas win on write: the writer uses its native namespace only for what has no neutral form, and regenerates or drops a native record that disagrees with the neutral model.

Native write-back of a partially decoded source is lossy and says so. Records that survive only as bytes cannot silently survive a write: those records may reference edited content that the writer cannot interpret.

## Three layers

The decoder returns `Decoded.ir`, losses, and source fidelity. The codec declares content scope before computing any digest. The wrapper checks invariants and adds missing-delivery losses. The report remains evidence about that decode.

The caller chooses policy. The CLI applies `--reject-lossy[=decode|export|any]` and `--reject-lossy-floor=info|warning|error|blocking`. Decode policy judges the native decode performed by this invocation, or the lossless CADIR parse. A restored sidecar describes the earlier decode and cannot decide rejection of this parse. An editor can display the same losses, let the user keep partial content or replace it, and save a re-bound sidecar.

The writer consumes the document. Its `REQUEST: DecodeDepth` tells a caller which decode request normally supplies its complete input. This is a request hint, not an admission gate. A synthesized write consumes neutral arenas and its own typed native namespace, without retained source bytes. Missing neutral scope produces a note, not a loss or refusal.

## Preconditions and policy

Every writer refusal has one of these classifications. A codec-local loss identifies each reduction or omission.

| Class | Meaning | Outcome |
| --- | --- | --- |
| P | A patch cannot express the edit | Synthesize; report `PatchDeclined { reason }` |
| S-arena | The target cannot carry a whole arena | Declare `Dropped` or `Omitted` and charge a loss |
| S-entity | An entity has no exact target encoding | Reduce or omit the entity; omit its body only when required for validity; structurally degraded output carries an Error loss |
| C | Malformed or inconsistent input | Refuse the precondition |
| O-capacity | A count, name, or other value exceeds target capacity | Refuse the precondition |
| O-target | The requested target cannot be resolved | Refuse the target |

An empty export is represented by a `NoExportableSolids` loss. The CLI writes empty geometry output with an `export:output.empty` Warning by default; `--reject-empty` refuses it. Check errors remain in the report and do not stop conversion unless `--reject-errors` is set. `--container-only` remains CLI policy and allows CADIR output only. A library writer does not infer these caller policies.

## Content scope

Wire version 10 requires a `scope` object with a boolean for every `ArenaClass`:

```json
{"scope":{"model":true,"design":false}}
```

`scope[C] = true` declares that the document holds class C. Its arenas are meaningful; empty arenas mean none was found or authored. `false` means the class is not held. Every neutral arena in that class must be empty. Classes are independent: a feature tree can hold design without model. A dumb solid can hold model and have complete, empty design.

Partial projection holds the class if some content was delivered; losses and `SourceFidelity.unrepresented` describe what was not represented. Partial projection is never a scope value. A totally failed class delivers the same document state as an omitted class. F3D design failure and its geometry cut both deliver model scope, without a failure arena. NX attachment failure clears design scope. Reports describe the different events.

Serialization, deserialization, `DecodeResult::new`, and `CadIr::set_scope` enforce that a populated class is held. `ScopeViolation { class, arena }` identifies the first violation. Dropping scope is atomic and refuses while an arena of that class is populated. An editor declares scope before authoring content into an excluded class.

`CadIr::empty()` has full scope: an authored document is complete by definition. Deliberately clearing design while retaining full scope means complete-and-empty design. Scope participates in `document_local_sha256`; source fidelity does not.

`DecodeDepth` is request vocabulary only. `Container.required_scope()` is empty, `Geometry` requires model, and `Full` requires model and design. A codec can over-deliver. `DecodeTransfer { scope, geometry_transferred }` derives scope from the document. The geometry boolean says whether any B-rep landed; an empty part can hold model without transferring geometry. `depth.not-delivered` names every required class that is absent.

## Typed content and source fidelity

`ArenaClasses` is an ordered set of arena classes, serialized as a sorted array. `SourceFidelity.unrepresented` defaults to empty and round-trips through the decode sidecar. It contains each class for which source records survive in no typed neutral or native arena: only retained bytes, opaque `unknowns`, unreadable property XML, or no representation at all.

A typed record must represent the record's content. An inventory of offsets, identifiers, raw cards, or diagnostic reasons does not represent missing geometry or design merely because the inventory itself is typed. Native diagnostics about retained records remain valid records; they do not infer neutral completeness.

The set is the union of the `untyped` classes of the retained records. A decoder marks a record where it retains the record without decoding it, with `RetainedSourceRecord::with_untyped` or `SourceFidelity::mark_untyped`. The set is never derived from a loss or from scope. FreeCAD's geometry cut retains readable design objects and properties as typed native records. A cut whose payloads are fully typed has no unrepresented design; opaque binary or Python payloads still add their content class. F3D's geometry cut creates no typed design records, so its set contains design. An unreadable FreeCAD design property's XML is raw content and adds design.

## Patch rule

Before patch or replay, `EncodeInput::ensure_represented` rejects nonempty `unrepresented` with `CodecError::UnrepresentedSource { format, classes, path, synthesized_available }`. The sealed encoder also checks the final path. Backends that choose their own path call the helper before consuming retained bytes. They can select synthesis instead. A patch admission failure selects synthesis and records `SynthesisCause::PatchDeclined`.

A patch must also meet the content-equality assertion in E1: **`assert_written_arenas_agree`** compares the decoded unedited patched output with the decoded source-less synthesized output on every arena the synthesizer claims `Written`. Source bytes cannot supply additional content in those arenas. Synthesis coverage must state supported, reduced, and omitted arenas honestly.

An editor that replaces design clears both its neutral design arenas and corresponding typed native design records and references. Deletion is an explicit edit, never an inference from a prior report. A geometry-scope document that retains typed native design still holds that native content.

The sidecar's hash binding is an artifact-flow integrity check. Saving edited CADIR requires `DecodeSidecar::bind_sha256` against the new bytes. The CLI ignores a digest-mismatched sidecar, reports `sidecar.stale` as a Warning, and uses the current CADIR document alone. Malformed sidecars and unsupported sidecar versions remain errors. Removing the sidecar leaves the CADIR document as the sole input.

## Extending the contract

A new codec sets `ir.scope` before any digest, declares `unrepresented` once when constructing its decode body, classifies every loss code, and tests success, cut, and contained failure. Its report scope must equal the document's scope. Missing required classes require `depth.not-delivered`. Golden depth checks verify that cuts preserve all content outside design and cannot make the unrepresented set smaller.

A new writer declares `REQUEST`, every neutral arena's disposition, and its own native coverage. It writes from neutral records first, can synthesize without fidelity, and guards every patch/replay route. It may refuse only C, O-capacity, or O-target preconditions; document those classes at refusal sites. It reports supported siblings when another entity must be omitted. Foreign native omission and own native omission use the shared export loss vocabulary.

Adding an arena class changes the wire contract and requires a wire bump. Update the classified arena registry, scope booleans and enumeration test, request mappings, invariant checks, hashing, codec declarations, loss classification tables, writer coverage, schemas, sidecars, CLI query/diff, conformance tests, goldens, seeds, and this document in one change. Every class boolean is required; unknown scope keys are rejected.

## Version policy

Readers accept exactly version 10. Earlier documents are refused with a message that names the change to `scope` and the report transfer change. There is no migration path in the reader.
