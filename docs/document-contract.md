# The document contract

CADIR is the truth. Decoders report; callers decide; writers write what the document holds.

A decode produces a document and a report. The report describes an event: what the decoder could not read, approximated, or skipped. The document describes a state: what it holds. Nothing in the document records why something is absent.

## Report, decision, write

The decoder returns `CadIr`, losses, and source fidelity. The codec declares content scope before computing any digest. The wrapper checks invariants and adds missing-delivery losses.

The caller chooses policy. The CLI applies `--reject-lossy[=decode|export|any]` and `--reject-lossy-floor=info|warning|error|blocking`. Decode policy judges the native decode performed by this invocation, or the CADIR parse. A restored sidecar describes the earlier decode and cannot decide rejection of this parse. An editor displays the same losses, lets the user keep partial content or replace it, and saves a re-bound sidecar.

The writer consumes the document. Its `REQUEST: DecodeDepth` tells a caller which decode request normally supplies its complete input. This is a hint for the request, not an admission gate. A synthesized write consumes neutral arenas and the writer's own typed native namespace, without retained source bytes. Missing neutral scope produces a note, not a loss or refusal.

## Preconditions and policy

Every writer refusal is one of the five `EncodeError` variants:

- `InvalidInput`: a validator-issued proof of a non-finite field the writer writes;
- `TargetCapacity`: a stored field exceeds a declared format capacity;
- `UnsupportedTarget`: the requested target cannot be resolved from the catalog;
- `ResourceLimit`: a caller resource allowance was exhausted;
- `Io`: a destination write or seek failed.

Policy stays with the caller. The CLI writes empty geometry output with an `export:output.empty` Warning and refuses it only under `--reject-empty`. Check errors stay in the report and stop conversion only under `--reject-errors`. `--container-only` allows CADIR output only. A library writer infers none of these policies.

## Scope and unrepresented

Wire version 10 requires a `scope` object with one boolean per arena class:

```json
{"scope":{"model":true,"design":false}}
```

`scope[C] = true` declares that the document holds class C. Its arenas are meaningful; empty arenas mean none was found or authored. `false` means the class is not held, and every neutral arena in that class is empty. Serialization, deserialization, `DecodeResult::new`, and `CadIr::set_scope` enforce that a populated class is held. `ScopeViolation { class, arena }` identifies the first violation. `CadIr::empty()` has full scope.

`SourceFidelity.unrepresented` is the union of the `untyped` marks of the retained records. A decoder marks a record where it retains the record without decoding it, with `RetainedSourceRecord::with_untyped` or `SourceFidelity::mark_untyped`. A decoder that decodes a record into typed fields does not mark it. The set is never derived from a loss or from scope. A nonempty set makes patch and replay ineligible: the wrapper withholds fidelity from the backend and the export carries `SynthesisCause::SourceUnrepresented { classes }`. Synthesis consumes no retained bytes. Retained bytes select layout and identities; they never supply content.

## Partial decode

A decoder gives up only the part it cannot read, decodes the rest, and reports what it gave up. A part is a unit the format frames by itself: an archive entry, a compressed stream, a record with its own length or terminator, a section. Each codec issue names its parts. A part the decoder reads that its codec issue does not name is given up with its nearest enclosing named part. When a part's end cannot be found, the decoder gives up the rest of its stream.

Each given-up part has one home in every decoder: the helper creates one `unknowns` stub for the part and keeps the part's bytes in fidelity as a `RetainedSourceRecord` for that stub. The stub id comes from the codec, the part kind, the stream, and the offset. The stub links the innermost record that contains the part. The helper marks the retained record `untyped` with the classes the part can hold, so they enter `unrepresented`. Nothing else in the document describes the failure. The decoder charges one `record.unreadable` loss (Error) per part, naming the part kind, stream, offset, length, record id, error text, and dropped records.

A record that contains a given-up part keeps the fields it read. A field whose bytes lie in the part is absent; in a positional list its entry is `null`, so later entries keep their positions. Records that need a dropped record are dropped transitively; records that list one lose only that entry.

Whole-file refusals are `WrongFormat`, `UnsupportedDialect`, a session-scope `ResourceLimit`, `Io`, and `Malformed` or `Truncated` when the decoder finds no part at all. A strict-mode rejection is the caller's rejection of a completed decode and is not a codec refusal.

Diagnostic records that restate a part give-up or unread bytes predate this home. Their removal belongs to the decoder units that own them (#222 IGES, #223 Rhino, #224 FreeCAD, #225 F3D, #227 Inventor, #229 Creo, #230 NX). The arena inventories name what the tree holds today.

## Out-of-scope classes

`scope[C] = false` holds when the decoder holds no record of class C and found no part of class C: the request excluded C, or no part of C could be located. A class whose parts were found but failed stays in scope; its failed parts are given up, and their classes enter `unrepresented`. A class for which the format defines no part is held whenever the request includes it, with empty arenas. A container-only decode delivers empty scope.

## Rules D, P, W, B, E

Rule D keeps one fact in one place. K1 duplication resolves by making one copy canonical and the other a typed reference to it, or by removing it. K2 evaluation caches declare their source in the IR, and `validate_neutral` checks their agreement. K3 independent facts that look alike keep one resolver each. The document never lies to pass a validator: a truthful document that fails validation indicts the validator.

Rule P projects every typed native record with a neutral home into that home, as a typed variant, a `Native` variant, or an added typed variant. A record with no neutral home stays native under Rule W. The link is the neutral record's `native_ref`. Native records keep their typed fields and carry no back-link. `Feature.ordinal` is dense source order from 0.

Rule W writes native records in three steps. W1 re-emits a typed record of the writer's own namespace that has no neutral counterpart when its references resolve; otherwise the writer drops it and charges `export:native.records-omitted`. W2 regenerates a native record with a neutral counterpart from the current neutral record when the projection disagrees. W3 declares `Written`, `Regenerated`, or `Dropped` for every native arena of the writer's namespace, with record overrides inside mixed arenas. The export report is the authority for the document and output.

Rule B keeps source layout out of neutral records. A neutral record holds no source byte offset, length, record size, or digest of source bytes. A native record keeps the source offsets and lengths its writer's patch path uses, as typed fields. The digest of the whole source is `decode_digest`.

Rule E stores evaluation staleness as `Feature.needs_recompute` on the wire. The edit helpers `set_dimension`, `set_parameter_expression`, and `set_feature_definition` mark the edited feature and its dependents; `set_dimension` on a driven parameter returns `DrivenParameter`. Decoder flags are FreeCAD object status Touched, Creo station-value disagreement, and SolidWorks `swConfigurationNeedsUpdate` for every feature; every other decoder reports false. A writer preserves the format's flag where one exists, else charges `export:evaluation.stale` naming the features.

## Residual bytes

A synthesized record carries document content authored from neutral and typed native records. Retained source bytes travel with the `unknowns` stub for their part and do not enter synthesized records. The per-record residual accounting of #232 §3.3 belongs to the writer units that own it (#233 STEP, #234 IGES, #235 Rhino, #236 F3D, #237 SLDPRT, #238 FreeCAD).

## Patch rule

Patch and replay are eligible with empty `unrepresented` and an unedited document. A patch that cannot express the edit selects synthesis and records `SynthesisCause::PatchDeclined { reason }`. The content-equality gate of #232 E1 and its test belong to the writer units that own them.

## New codecs, writers, classes, parts

A new codec sets `ir.scope` before any digest, declares `unrepresented` where it retains records without decoding them, classifies every loss code, and tests success, cut, and contained failure. Its report scope equals the document's scope. Missing required classes charge `depth.not-delivered`.

A new writer declares `REQUEST`, every neutral arena disposition, and its own native coverage. It writes from neutral records first, synthesizes without fidelity, and guards every patch and replay route. It refuses with one of the five `EncodeError` variants and documents those classes at the refusal sites. It reports supported siblings when another entity must be omitted.

Adding an arena class changes the wire contract and requires a wire bump. Adding a part kind names the part in the codec's issue with its framing, stream, and give-up test.

## Worked example

A dimension is a literal or a binding: `Dimension<Length>::Literal(Length::new(10.0))` holds its own value, while `Dimension<Length>::Parameter(id)` names the `DesignParameter` whose cached `value` supplies it. `CadIr::dimension_value(&dimension)` resolves either form against the parameter arena. `set_dimension(&feature, "/profile/depth", Length::new(12.0))` edits the bound parameter through the feature definition path; on a driven parameter it returns `DrivenParameter` with the drivers instead of overwriting the value.

## Version policy

Readers accept exactly version 10. Earlier documents are refused with a message that names the change to `scope` and the report transfer change. There is no migration path in the reader.
