<!-- SPDX-License-Identifier: CC-BY-4.0 -->
# cadmpeg state machines

This file is the inventory of explicit and implicit state machines in cadmpeg: states, invariants, and transition tables. A machine here is an ordered lifecycle with guards. Closed value sets and payload enums stay out unless they drive those lifecycles.

Evidence is current as of `t3/state-machine-audit` on `origin/main`. Confirmed reachable illegal states are in [Reachable defects](#reachable-defects). Related fixes are grouped into issues in [Issues](#issues). Applied fixes are marked **fixed**.

## How to read a table

| Column | Meaning |
| --- | --- |
| From | Logical state before the event |
| Event | Call, byte, or policy that fires |
| Guard | Condition that selects the row |
| To | Next state |
| Else | Failure or refuse path |

`Explicit` means a named enum or type-state. `Implicit` means a protocol the types allow but do not name.

## Product pipeline

### Command report and process exit

**Implicit.** Two orthogonal machines: wire `CommandStatus` (`ok` \| `refused`) and process exit `0` \| `1` \| `2`. Sites: `crates/cadmpeg/src/reporting.rs`, `crates/cadmpeg/src/refusal.rs`, `crates/cadmpeg/src/main.rs`.

**Invariants**

- Wire `status` is `ok` or `refused`.
- `refused` carries `refusal.{stage,code,message}`.
- Exit 0 is success. Exit 1 is a semantic refusal. Exit 2 is operational (`Io`, unrecognized input, `decode_failed`, `resource_limit`, `binary_stdout_rejected`).
- Report write failure keeps the original command exit (`commands.rs` `write_refusal`).

| From | Event | To (exit) | Report |
| --- | --- | --- | --- |
| running | command completes | 0 | `ok` |
| running | `CheckFailed`, loss policy, unsupported dialect/target, strict decode, insufficient depth | 1 | `refused` |
| running | `diff` models or geometry disagree | 1 | `ok` with `different: true` |
| running | I/O, `DecodeFailed`, resource limit, binary stdout | 2 | `refused` when typed |

`diff` reporting `ok` with exit 1 is the documented verdict split. Measure depth refusals reuse `insufficient_depth` with wire `stage: plan`.

### Convert stages

**Implicit.** Load → decode → check → plan → write. Wire `RefusalStage`: `plan` \| `decode` \| `check` \| `export` \| `measure`. Sites: `crates/cadmpeg/src/transcoder.rs`, `artifact_store.rs`.

| From | Event | Guard | To | Else |
| --- | --- | --- | --- | --- |
| start | resolve target / destination | existing output and no `--force` | refused before decode | continue |
| open | `load_artifact` | native / CADIR / sidecar | `Decoded` / `Restored` / `Neutral` | operational error |
| decoded | `--reject-lossy` decode | any decode loss | `decode_loss_rejected` | continue |
| decoded | `validate_loaded` | error findings and no `--allow-errors` | `check_failed` | continue |
| checked | `encoder.plan` | plan `Err` and carried errors | `check_failed` preferred | plan refusal |
| planned | export losses and reject-lossy | nonempty export losses | `export_loss_rejected` | write |
| write | `NamedTempFile` then rename | persist fails | operational error | `ok` |

`--allow-errors` writes after check errors. CADIR and sidecar use two independent renames; a crash between them leaves a digest mismatch and the next load fails closed.

No CLI retry loop exists.

### Decode session (`DecodeContext`)

**Implicit** session plus explicit `DecodeDepth` (`Container` < `Geometry` < `Full`) and `DecodeTransfer`. Sites: `crates/cadmpeg-core/src/decode/`, `crates/cadmpeg-ir/src/codec.rs`.

**Invariants**

- Requested depth is immutable for the session.
- Budget fuse is sticky. `finish_session` resurfaces it.
- `DecodeContext` is `!Sync`. Parallel work snapshots a `DecodeCheckpoint`, resumes on a private arena, and commits with `accept_trace` on the owning thread.
- `DecodeResult::new` stamps `CadIr.decode_depth` from the transfer and refuses design arenas below `Full`.

| From | Event | To | Else |
| --- | --- | --- | --- |
| empty | `new` / `for_loaded_input` / `read_root` | input admitted | resource refuse |
| live | charge over ceiling | fused | same `ResourceLimit` on later charges |
| live | `checkpoint.resume` | tracing task | independent counters |
| tracing | `take_trace` then `accept_trace` | parent updated | parent unchanged; caller re-executes |
| live/fused | `finish_session` | closed | fused error |

`accept_trace` false is all-or-nothing. Dropping an `ExpandWriter` after partial writes keeps the decompressed charge.

### Dialect layers and admission

**Explicit.** `Admission`: `Admitted` \| `Unverified { using }` \| `Residual` \| `Refused`. `DialectLayers`: one primary plus extras unique by `(format, instance)`. An extra in the primary format carries an instance. Sites: `crates/cadmpeg-core/src/dialect.rs`. Architecture: [architecture.md](architecture.md) “Every classified report has one primary layer.”

| From | Event | Guard | To | Else |
| --- | --- | --- | --- | --- |
| empty | `of(primary)` | — | `{primary, []}` | — |
| set | `insert(layer)` | same `(format, instance)` | — | `Duplicate` |
| set | `insert(layer)` | primary format and empty instance | — | `MissingInstance` |
| set | `insert(layer)` | distinct key | extras extended | — |
| wire | deserialize | insert fails | — | read error |

Registry `read` in `dialect-support.toml` is a capability column. Golden tests require it to match `Admission`. `read = "refused"` carries a checker-only `refusal` cause.

## CadIr document

### Draft, commit, finalize

**Mixed.** `ModelDraft<()>` vs `ModelDraft<DraftAccounting>` is type-state. Document lifecycle is implicit. Sites: `crates/cadmpeg-ir/src/{lib.rs,draft.rs,document.rs}`.

**Invariants**

- Draft `insert` refuses duplicate identities inside the draft.
- Commit validates identity, references, and feature parents before mutating the destination. A refused draft leaves the base unchanged.
- `finalize` sorts arenas when unordered and is idempotent.
- `DecodeResult::new` finalizes. Draft `commit*` appends without sorting.

| From | Event | To | Else |
| --- | --- | --- | --- |
| — | `CadIr::empty` / `decoded` | empty / decoded shell | — |
| empty | populate / `commit*` | mutable, often unsorted | `DraftError`, base unchanged |
| mutable | `finalize` | identity-sorted | — |
| any | `validate_neutral` | same document + `ValidationReport` | `CodecError` aborts the report |
| any | `to_canonical_json` | JSON (clone is finalized) | non-finite float |
| JSON | `from_json` | CadIr, order unchecked | version / design-below-full |

`CommitSession` holds an exclusive borrow. `ModelCheckpoint` truncates suffixes on discard. In-place prefix edits survive discard if a caller breaks the append-only contract.

### Validation report

**Implicit** observation gate. Site: `crates/cadmpeg-ir/src/validate.rs`. Findings use `Info` < `Warning` < `Error` < `Blocking`. `is_ok()` is `error_count == 0` and ignores `losses`. `admit*` filters by allowed `Check`s. Incremental `AdmittedState` is a Rhino cache; a hard `CodecError` drops it.

### Occurrence placement

**Explicit.** `Resolution`: `Pending` → `Active` → `Resolved(Transform)`. Site: `crates/cadmpeg-ir/src/products/assembly_graph.rs`. Re-entry of `Active` is a cycle. Missing parent and non-finite compose fail the graph. Failed resolve discards the local map.

### Feature suppression and configuration evaluation

**Implicit** dual view. `Feature.suppressed: Option<bool>` and `ConfigurationEvaluation::{Suppressed, Active}`. `Feature::configuration_state` treats `None` as active (`unwrap_or(false)`). Sparse `feature_states` inherit model-level state. An **active** configuration’s suppressed set must agree with that model-level bit (**fixed**: `None` now participates).

### Geometry carriers and transforms

**Explicit** `SurfaceGeometry` / `CurveGeometry`: `Procedural { cache }` \| `Solved`. Cache layouts: `Legacy` \| `Revision`. A `Transform` is a finite affine 3×4; singular and improper matrices are representable. Placement nesting depth is `0..=256`.

### Export write path and fidelity

**Explicit.** Backend `WritePath`: `VerbatimReplay` \| `Patched` \| `Synthesized`. Fidelity: `NotProvided` \| `NotConsumed` \| `Replayed` \| `Degraded`. `into_report` collapses by `fidelity_provided`. Verbatim cannot be `NotConsumed`. Synthesized cannot be `Replayed`.

Strict decode evaluates `LossNote::strict_consequence` once in `DecodeResult::new` for non-container Strict mode. CLI `--reject-lossy` counts every loss, including Info.

## Topology and measurement

`RadialStatus` (`Closed(n)` \| `DoesNotClose` \| `CrossesEdge`) is the topology-graph classifier in `validate/topology/graphs.rs`. `Sense` is a static bit.

Measurement `RowStatus` is derived (`Measured` \| `Partial` \| `Unavailable`). Face orientation `Verdict`: `Settled` \| `Confirmed` \| `Conflict` \| `Unconfirmed` → witness → `Confirmed` \| `Contradicted`. Codecs do not depend on `cadmpeg-measure` in production; the crate is an oracle.

## Container parsers

### ZIP (`cadmpeg-container`)

**Implicit.** Directory admit → optional local recovery → member open. Ambiguous EOCD and duplicate names refuse. Missing directory recovers from local `PK\x03\x04` frames. Encryption and unsupported compression admit in the snapshot and refuse at `open`. Resource limits never recover.

### CFB / OLE

**Implicit.** Full structural parse. Almost every defect refuses. DIFAT `FREESECT` terminator may admit with `CompoundDeviation`. Empty names refuse. No stream recovery analogous to ZIP local frames.

## Codec session machines

Each codec owns a decode route. Depth cuts skip design projection below `Full` unless noted. Write paths follow the export machine above.

| Codec | Machine | Notes |
| --- | --- | --- |
| STEP | parser exchange HEADER→DATA→graph omission | Syntax omits a statement; resource/lex refuse. No retry. |
| IGES | section order `S<G<D<P<T`; `EntityStatus`; `TransferLedger.verify` | Ledger failure is `CodecError::malformed`. |
| SAT | host framing + kernel save-format band | Unframed host → `UnsupportedDialect`. |
| SLDPRT | carchive `End`/`Stop`; tessellation token agreement; write path | `SurfaceTokenStatus::Invalid` is dropped before storage. |
| NX | OM payload classify; `BodyCensusEvaluation`; `nx_profile` CLI dual-worker | CLI statuses are tooling. |
| CATIA | variant identify → dialect → ordered routes → metadata fallback | V5 only. `Unknown` is residual. |
| Creo | layout/dialect; `PcurveCarrierStatus` | Residual layout recovers. |
| Inventor | RSe pair/meta/bulk; design skip below Full | Container keeps non-design arenas (depth harness). |
| F3D | geometry-depth cut with promotion to Full; three write paths | Archive, Canvas/Decal, bodyless-without-source promote. |
| FreeCAD | depth cut without promotion; always `Patched` write | `ProgramVersion` is metadata. |
| Rhino | `ChecksumStatus`; object `TrailerState`; table-scan recovery | Soft checksum/object framing; ranked tables hard-order. |
| USD | layer BFS load; stage compose; 4 variant-selection passes | Unsettled selection keeps last sites. |
| PVZ | dialect residual; ZIP recovery; exact-stream Faces→Curves | Stream order errors are hard. |
| Protein | page framing; schema inheritance; per-record accept/reject | Resource aborts the session. |
| Parasolid | deltas `NodeState`/`Expect` | Illegal packets end `DeltasEnd::Stopped`. |
| Tessellate | bounded refine/project loops | Caps can return `Ok` under remaining error. |
| ASM | — | B-rep decoder; product assembly lives in IR. |

## Retry and concurrency

Production code has no Rayon/`par_iter`/`select!`. Shared mutation is:

- F3D `ContainerScan` `Mutex` cache: last equivalent `Arc` wins; poison recovers with `into_inner`.
- F3Z `thread::scope` plus `DecodeCheckpoint` / `accept_trace_rebinding`; composition refuses foreign origins.
- NX support-UV retries keyed on prior lane state; completed lanes stay.
- `OnceLock` memoizes immutable data.

No retry or concurrent path was found that commits an illegal machine transition onto shared state.

## Reachable defects

Ranked by user-visible impact. “Reachable” means a constructor, deserializer, or decode path in this tree.

| Rank | Machine | Defect | Evidence | Issue | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | `DialectLayers` | Tagged primary plus untagged extra in the same format was constructible and deserializable. Architecture requires an instance on that extra. | `dialect.rs` `insert` / `same_key`; `architecture.md` primary-layer invariant | [#1021](https://github.com/cadmpeg/cadmpeg/issues/1021) | **fixed** |
| 2 | Feature / configuration | Active-configuration suppression compared `Feature.suppressed` only when `Some(_)`. `None` is active in `configuration_state` (`unwrap_or(false)`), so `None` plus a suppressed config row was accepted. | `validate/topology.rs` former `is_some_and`; `features.rs` `configuration_state`; `features/tests/configurations.rs` | [#1022](https://github.com/cadmpeg/cadmpeg/issues/1022) | **fixed** |
| 3 | F3D history | `check_transitions` requires `previous_state_id` to match the `next_ref` predecessor. `face_changes_across_state_chain` still walks `transition.previous_state_id` and applies stored deltas. Callers that skip `check_transitions` can aggregate the wrong faces. | `history_records/chain.rs` `bound_chain`; `history.rs` `face_changes_across_state_chain` | [3](#issue-3-remaining-codec-machines) | open |
| 4 | Creo `DefinitionIdentity` | `Parsed { schema_id: None, owner: Some(n) }.id()` is `0`. `BoundOwner { schema_id: None, owner_feature_id: n }.id()` is `n`. `bind_section_owners` emits both spellings. Joins that key on `id()` diverge. | `feature/definitions.rs` `id` / `bind_section_owners` | [3](#issue-3-remaining-codec-machines) | open |
| 5 | CADIR + sidecar | Two atomic renames, no joint transaction. Crash between them yields a stale sidecar; load fails closed. | `artifact_store.rs` module docs | [3](#issue-3-remaining-codec-machines) | documented limitation |

### Examined and kept

| Claim | Result |
| --- | --- |
| Inventor container transfers B-rep | Depth harness: non-design arenas match Full. Container bulk frames stay `Unavailable` without a kernel stream. README “does not inflate bulk geometry streams” refers to those frames. |
| FreeCAD `TopologyOccurrence.source_index` starts at 1 | Bind tests pad `names[0]` and index from 1. Legacy maps pad to the parsed index. |
| `validate_neutral().is_ok()` ignores `losses` | Documented: findings vs decode losses are separate. |
| Measurement feeds codecs | Production codec crates list `cadmpeg-measure` only in `[dev-dependencies]`. |
| Mutex / retry races into illegal states | No conflicting transition found. |

## Issues

Related fixes share an issue. [#1021](https://github.com/cadmpeg/cadmpeg/issues/1021) and [#1022](https://github.com/cadmpeg/cadmpeg/issues/1022) are implemented in this change. Issue 3 stays open.

### Issue 1: Dialect layer identity

Enforce the architecture rule at `DialectLayers::insert` and serde: an extra layer in the primary format must carry a nonempty instance. Keep `(format, instance)` uniqueness. Untagged extra versus untagged primary remains `Duplicate`.

Regression: `dialect_layers_reject_an_untagged_extra_in_a_tagged_primary_format` and the matching deserialize test in `crates/cadmpeg-core/src/dialect.rs`.

### Issue 2: Configuration suppression

Treat `Feature.suppressed == None` as not suppressed when an active configuration lists a suppressed state, matching `Feature::configuration_state`.

Regression: `features/tests/configurations.rs` active configuration with `suppressed: None` and a suppressed `feature_states` row.

### Issue 3: Remaining codec machines

F3D history walkers that follow `previous_state_id` independently of `next_ref`, and Creo `DefinitionIdentity::id` for schema-less parsed owners, need codec-local follow-up with their own fixtures. The CADIR/sidecar pair is a known crash window; closing it is a persist redesign.
