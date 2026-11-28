# Decode resource caps

This guide gives the kinds of limit that bound a decode and the result of each
kind at exhaustion. Use it before you add a bound. Do not add a
`ResourceLimits` field for a codec-local limit.

`ResourceLimits::desktop()` and `ResourceLimits::service()` in
`crates/cadmpeg-core/src/decode/policy.rs` own the profile values and the
proportional-allowance constants. This guide does not restate them. If a
note here and `policy.rs` disagree, `policy.rs` is correct.

## Policy kinds

Each limit has one kind. The kind comes from the result at exhaustion, not
from the name of the constant.

1. **Format or reader invariant.** The format or the reader states the
   limit: a field width, a fixed record shape, a framing window, an admitted
   degree. A value outside the limit is a malformed or unsupported form, or a
   candidate that does not match. No profile changes the limit.
2. **Per-input bound.** The limit bounds a count, a size, or a nesting depth
   that one input states, before the decoder allocates, copies, or expands.
   Where the bytes carry the items, check the count against the remaining
   bytes (`bounded_len`, `View::counted`, `take`, or a codec helper such as
   Rhino `checked_count_bytes`). Excess refuses that record or that decode as
   malformed or unsupported, or skips that record. A per-input bound does not
   set the session fuse.
3. **Session ceiling.** Only the `ResourceLimits` fields are session
   ceilings. `DecodeContext` charges six of them: `max_input_bytes`,
   `max_decompressed_bytes_total`, `max_decompressed_bytes_per_expand`,
   `max_retained_bytes`, `max_entities`, and `max_recursion_depth`. Excess
   refuses the session and sets the fuse. Reading a document enforces
   `max_state_chain_depth` on each chain of state bases its evaluations
   and process states form, and refuses a cycle as malformed. Excess refuses
   that read with `CodecError::ResourceLimit` (`RecursionDepth`,
   `BudgetExceeded`); it sets no fuse. The depth of a document and of an
   expression are bounds of the CADIR wire (§2.5), `MAX_DOCUMENT_DEPTH` and
   `MAX_EXPRESSION_DEPTH`, and no session ceiling.
4. **Local work cap.** The limit bounds a search, an iteration, a
   convergence loop, a walk over derived data, or the size of derived output.
   A `WorkBudget` or a local counter enforces it. At exhaustion the decoder
   skips that item and reports the skip as a loss. A local work cap never
   refuses the session. A cap may have a fixed size only when it applies to
   one item: one sketch, one surface, one stream, one search. A counter
   shared across a whole decode, or across every item of one kind, must
   scale with the input, for example k times the number of rows it walks,
   with a fixed floor. A fixed shared counter makes a larger model lose
   content that a smaller one keeps.

The census also uses **N** for a constant that is not a decode bound: a
numeric representation limit, an encode-path or CLI limit, or the capacity
of a cache whose exhaustion does not change the output.

## Exhaustion outcome

A session ceiling refuses. `DecodeBudget::charge` records the refusal and
sets the fuse. Every later charge fails with the same refusal, and
`DecodeContext::finish_session` returns it even when codec code swallowed
the error. The refusal names a `ResourceDimension` (`InputBytes`,
`DecompressedBytes`, `RetainedBytes`, `Entities`, or `RecursionDepth`) and a
`ResourceFailure` (`BudgetExceeded` or `AllocationFailed`). An allocation
failure in `alloc_filled`, `concat_views`, `concat_retained`, or the
Zstandard decoder fuses the session as `RetainedBytes` / `AllocationFailed`.
Only `cadmpeg-core` sets the fuse; codecs reach it through `DecodeContext`
charges and `refuse_input_limit`.

A local work cap skips. The caller checks `WorkBudget::exhausted` (or its own
counter), drops the item it was building, and adds a loss that names the
bound. A partial result is not emitted as complete. Use
`WorkBudget::charging_slice` for a nested phase: the child limit is the
smaller of the requested limit and the parent's remaining work, every charge
to the child also charges each counter it was drawn from, and exhausting the
child leaves the parent usable. The work below a counter therefore never
exceeds its limit, however deeply phases nest or repeat. `child_slice` makes
a detached child whose work reaches the parent only when `consume_child`
moves it back; use it only for a phase that runs alone and is transferred
before the parent charges again.
`WorkBudget::recursion_guard` bounds recursion inside a local search; at its
depth it exhausts the work budget and does not touch the session.

A per-input bound refuses or skips one record, or refuses the decode as
malformed or unsupported. It does not fuse the session, so the decode can
continue with other records when the codec allows it.

Do not use saturating arithmetic as a limit. Use checked arithmetic and treat
overflow as excess.

## Proportional allowances

Three session ceilings also have an input-proportional allowance:
decompressed bytes in total, decompressed bytes per expansion, and retained
bytes. The allowance is the smaller of the ceiling and a base plus a
per-input-byte amount times the input length. An overflow in that sum gives
the ceiling. Input bytes, entities, and recursion depth use the ceiling
directly.

`begin_expand` checks a declared expansion size against the per-expansion
allowance and the remaining total allowance, and `ExpandWriter::write`
charges each chunk. `open_zstd` checks each frame's window before it
allocates a decoder: a window above the per-expansion allowance is a
per-input bound, so the stream is `NotImplemented`, the session is not
fused, and the caller skips that member and reports a loss. The bytes a
member actually expands still go through `begin_expand` and `write`. A
codec that reads a declared size from the input compares it with
`DecodeContext::per_expand_allowance()` before `begin_expand` and skips the
member with a loss when it is above, as Creo does for an LZW section.
Processing of an input that is already loaded starts
with `DecodeContext::for_loaded_input`, which takes the measured input
length; a format-declared count is not an input length. A codec limit that
takes `min` with a session dimension takes it with the allowance, not with
the raw ceiling.

## Retained bytes

Retained bytes are charged only at `DecodeContext::retain_bytes`. The
chokepoints are:

- native record text: `NativeRecord` serialization for decode and
  unknown-record attachment in `cadmpeg-ir`;
- arena buffers: `concat_views` and `concat_retained` in `cadmpeg-core`;
- retained copies of source or decoded bytes in codecs, for example source
  record bytes, logical streams, mesh channels, embedded images, and
  Parasolid streams.

Expansion output from `begin_expand` is charged as decompressed bytes, not
again as retained bytes. Parse trees, indexes, caches, and other working
storage are not retained bytes; the expanded-bytes charge of an XML or JSON
parse, per-input bounds, entity charges, and local work caps bound them. A new
copy of bytes that the decoded document keeps must call `retain_bytes`
before the copy. A native record's text may be
counted twice toward retained bytes, once by the codec before attach and
once by the IR at attach (Rhino link bytes and `from_typed_for_decode`
JSON copies), so the retained ceiling is conservative by at most that
text.

`parse_xml`, `parse_json_value` and `parse_json` scan the text before they
parse it and charge its depth to the session. The depth is charged before
the parser runs, because roxmltree recurses once per element level.

For XML, the scan counts what roxmltree 0.21.1 allocates: the node and
attribute slots that it reserves for every `<` and `=` in the text, the
texts and attribute values that it copies (those with references, carriage
returns, CDATA, tabs or line breaks), the merged text pieces, the per-level
parser state, the temporary attributes of the widest tag, the namespaces,
and three node slots for each node it can make: the root, one for each
`<`, and one text node after each `<`. It also counts the name comparisons
that roxmltree makes, one byte each: each attribute with the earlier
attributes of its tag, and the name of each tag and prefixed attribute with
the namespaces in scope. The session charges that count to the
decompressed-bytes dimension before roxmltree parses, against the caller's
`max_decompressed_bytes_total` with no allowance that scales with the
input, and adds it to the other expanded bytes of the session. The
scan's set of namespace prefixes is admitted against the same ceiling as it
grows. A refusal is `ResourceLimit` of `scope: Session` whose `additional`
is the count, `u64::MAX` where the count passes `u64`, and it sets the
fuse. For a text that `parse_input_xml` reads, each attribute, a namespace
declaration included, and each distinct namespace prefix is an entity
charged to the session before roxmltree parses.

A JSON tree is estimated from its delimiter counts as a `serde_json::Value`
tree, also for typed parses. The session charges that estimate to the
decompressed-bytes dimension before the parse, against the same ceiling as
an XML parse, and a refusal is the same `ResourceLimit` with the estimate
as `additional`.

## Retained-byte availability

`DecodeContext::retained_bytes_available()` returns the retained-byte
allowance minus the bytes already retained, or zero after the fuse is set.
It does not charge and it does not set the fuse.

Use it only to decide, before you build it, whether to skip optional
enrichment. Report the skip as a loss. The enrichment that is built is still
charged through `retain_bytes`, which can refuse. Do not use the query to
size, truncate, or skip mandatory output.

F3D history binding is the first user. `decode_design_graph` reads the
availability once and passes it to the history decoder.
`admit_history_binding` subtracts an estimate for each history stream
(changed record slots times `HISTORY_RETAINED_BYTES_PER_CHANGED_SLOT`, the
estimated `asm_delta_states` native text per slot). A stream whose estimate
does not fit is not bound and keeps only its state headers, without bulletin
boards or history records, and the decode reports
`history.binding-budget-exceeded`. The native records of a history are
charged when they are serialized. Binding a stream's topology then charges
the records it decodes to `MAX_HISTORY_BINDING_WORK`, shared by the streams
of the decode; a stream that exhausts it keeps its bulletin boards and
history records without historical topology, and the decode reports
`history.binding-work-exceeded`.

## Scratch decodes

`DecodeContext::scratch` runs optional evidence work in a separate arena
and resource account. The measured carrier length scales its allowances.
The codec's work and population caps bound repeated requests. The decoder
admits the bytes and entities of each kept result together. A refusal
at that admission leaves the session usable.

An F3D or Inventor history nest admits its records and body and face names.
An Inventor state view admits the topology, naming attributes and body keys
that its proofs read. A profile body admits its wires and copied spline
coefficients. An identity lookup, source key check or Loft body kind probe
keeps a scalar result and discards the temporary graph.

A resource refusal gives up the affected evidence. The codec reports the
limit on a bounded history body, Base source body, Loft result or unread
profile body. `cadmpeg_asm::brep::inspect_with_header` and
`inspect_with_table` read framed records and preserve the shell membership
marks. The Model path copies withheld and passthrough spans and emits
provenance records.

Scratch work uses the same evidence allowance at every F3Z composition
position. The whole-session trace composes the charges of the kept output.

## Scan limits

`ScanLimits` in `crates/cadmpeg-core/src/scan/mod.rs` bounds a feature-only
design scan. A `ScanBudget` meters the whole scan, nested documents
included, and a charge is cumulative. Exhaustion stops the scan with the
matching `StopReason`; the scan keeps the rows admitted before the stop and
reports the census as incomplete. `ScanLimits::default()` sets:

| Field | Default | `StopReason` |
| --- | --- | --- |
| `source_bytes` | 16 MiB | `SourceBytes` |
| `expanded_bytes` | 32 MiB | `ExpandedBytes` |
| `retained_bytes` | 64 MiB | `RetainedBytes` |
| `elapsed_time` | 5 s | `ElapsedTime` |
| `feature_count` | 100 000 | `FeatureCount` |
| `nesting_depth` | 32 | `NestingDepth` |

Parser storage and result storage charge `retained_bytes` before the
allocation, and `ScanBudget::release_retained` returns a charge for storage
that the scan dropped. A scan limit is a host ceiling, not a `ResourceLimits`
field.

## Production classifications

Each production `const MAX_*` has a `// resource-cap: MAX_NAME = KIND`
marker on the line before its declaration. The marker states one of the kinds
above. `python3 scripts/check-resource-caps.py` checks the markers and prints
the counts by kind and crate.

### Work budgets without a named `MAX_*` limit

These production `WorkBudget::new` sites take a limit that is not a `MAX_*`
constant. Sites that forward a limit parameter from a `MAX_*` constant
(the CATIA mesh-quotient face slice, the zero-entity candidate search, and NX
`GeometryWorkBudget::from_context`) are classified with that constant.
`WorkBudget::child_slice` and `WorkBudget::charging_slice` in `cadmpeg-core`
are the slice mechanisms themselves.

| Crate                  | File                                                             | Limit                                  | Value                                        | Enforcement                                                                                                                                  |
| ---------------------- | ---------------------------------------------------------------- | -------------------------------------- | -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `cadmpeg-codec-f3d`    | `src/design/decode/operands.rs` (four sites)                     | `RECIPE_SIDE_SEARCH_WORK`              | `1 << 22`                                    | Edge and face recipe side search. Exhaustion reports `feature.recipe-search-budget-exceeded`, which names the constant.                     |
| `cadmpeg-codec-f3d`    | `src/design/decode/scopes/pattern.rs`                            | `RECTANGULAR_PATTERN_RUN_WORK`         | `1 << 20`                                    | Rectangular pattern run search; the constant is in `src/records/feature/patterns.rs`. A capped search reports `feature.pattern-instance-search-capped`, which names the constant. |
| `cadmpeg-codec-f3d`    | `src/design/feature_project.rs`                                  | `PARAMETER_ORDER_WORK`                 | `1 << 22`                                    | Parameter ordering. Exhaustion reports `parameter.order-budget-exceeded`, which names the constant.                                         |
| `cadmpeg-codec-f3d`    | `src/design/sketch_project.rs`                                   | `SPATIAL_PROFILE_SEARCH_WORK`          | `1 << 24`                                    | Spatial sketch profile search. A capped sketch has no closed profiles and `sketch.profile-search-capped`, which names the constant, is reported. |
| `cadmpeg-codec-nx`     | `src/decode/build.rs`                                            | `transfer_limit`                       | 8 per chart, clamped to 1_024..=65_536       | Model-wide opposite-chart transfer budget (`completion_transfer_budget_limit`). Exhaustion reports `intersection.pcurve-completion-bounded`. |
| `cadmpeg-codec-nx`     | `src/decode/build.rs`                                            | `support_uv_limit`                     | 8 per chart, clamped to 1_024..=65_536       | Serialized support-UV validation budget. Same loss.                                               |
| `cadmpeg-codec-creo`   | `src/container.rs`                                               | `data.len()`                           | input length                                 | TOC header and row bytes visited across all `#UGC_TOC` headers. Exhaustion stops the TOC scan and reports `container.toc-scan-capped`.      |
| `cadmpeg-codec-creo`   | `src/decode/sweep/profiles.rs`                                   | `PROFILE_VALIDATION_WORK_PER_PIECE`    | 256 × pieces, at least `1 << 22`             | Pair sweep, containment and NURBS self-intersection work of one extrusion profile set. Exhaustion rejects the set with a lane refusal naming the constant, reported as `topology.extrusion-body-rejected`. |
| `cadmpeg-codec-sldprt` | `src/brep/evaluation.rs` (two sites)                             | `SURFACE_SOLVER_LOCAL_WORK`            | `1_000_000`                                  | NURBS surface inversion and chord bound. Exhaustion skips the pcurve; one `geometry.surface-solver-work-capped` note counts the coedges.     |
| `cadmpeg-codec-sldprt` | `src/parasolid.rs`                                               | `PROBE_WORK_PER_PAYLOAD_BYTE`          | 16 × payload length                          | Charges the remaining payload at each zlib header candidate. Exhaustion ends the probe and reports `container.parasolid-probe-capped`.       |
| `cadmpeg-codec-sldprt` | `src/tessellation.rs`                                            | `TESSELLATION_OWNER_WORK`              | `max(1 << 30, (1 << 17) × rows)`             | Surface-owner trims, containment and vertex measures; rows are faces, coedges, and the vertices and triangles of unowned tables. Exhaustion leaves the rest unassigned; `tessellation.owner-work-capped` counts them. |
| `cadmpeg-codec-sldprt` | `src/pmi.rs`, `src/pmi/patch_slots.rs`                           | `PMI_PARSE_WORK_PER_PAYLOAD_BYTE`      | 16 × payload length                          | One unit per MessagePack value and validated string byte. Exhaustion skips the rest of the section; `pmi.semantic-parse-capped` counts them. |
| `cadmpeg-codec-sldprt` | `src/swift.rs`                                                   | `SWIFT_FEATURE_WALK_WORK`              | `max(1 << 24, 1024 × rows)`                  | SWIFT feature walks of one projection; rows are the root's features and annotations. A walk cut short stays unresolved; `pmi.swift-feature-walk-capped` counts them.                       |
| `cadmpeg-codec-sldprt` | `src/history/configuration.rs`                                   | `CONFIGURATION_PROJECTION_WORK`        | `max(1 << 26, 64 × rows)`                    | History records copied per configuration; rows are history records and configurations. Later configurations stay empty; `config.projection-work-capped` counts them.                      |
| `cadmpeg-codec-catia`  | `src/solve/mesh_quotient/coordinate_assignment.rs` (three sites) | `budget.remaining()`                   | parent remainder                             | Charging slices of the caller's budget; exhausting one skips that scan without exhausting the caller.                                        |
| `cadmpeg-codec-catia`  | `src/solve/mesh_quotient/coordinate_assignment.rs`               | `component_limit`                      | derived from `MAX_COORDINATE_CLOSURE_STATES` | Per-component closure budget: support and traversal work times the state limit, as a charging slice of the caller's budget when it has one. |
| `cadmpeg-ir`           | `src/eval.rs` (two sites)                                        | `DEFAULT_NURBS_SURFACE_INVERSION_WORK` | `1_000_000`                                  | Surface inversion entry points without a caller budget. The callers own skip and loss.                                                       |
| `cadmpeg-ir`           | `src/eval/admission.rs`                                          | `parent.remaining()`                   | parent remainder                             | `within_work_slice` child; `consume_child` charges the parent and an exhausted child exhausts the parent.                                    |

## Routing

Route a new bound as follows.

- A count or size read from the input is kind 2. Check it against the bytes
  that carry it before you allocate, and allocate with `alloc_filled`.
- Recursion over input structure charges `enter_nested`. Add a local depth
  constant only when the format states a depth.
- Compressed data goes through `begin_expand`. A codec-local expansion limit
  is kind 2 and is checked before `begin_expand`.
- Bytes that the decoded document keeps go through `retain_bytes`.
- Entities are charged with `admit_entities` at admission boundaries or with
  `charge_entities`.
- A search, a solver, or a derived walk gets a named `WorkBudget` limit and
  `charging_slice` for nested phases. At exhaustion it skips the item and
  adds a loss.
- Optional enrichment whose size is known before it is built may check
  `retained_bytes_available()` and skip with a loss.

Current codec routes:

- STEP parse nesting, anchor and REFERENCE expansion, the reference walk,
  and the style, PMI, unit and pcurve graph walks charge `enter_nested` and
  have no local depth constant. The style, PMI and invisible-body walks keep
  a visited set or a result per record, so one walk visits each shared
  record once. The tessellation association walk follows a record
  once per distinct inherited placement, up to `MAX_ITEM_PLACEMENTS` per
  item. The anchor and REFERENCE node counts, and the payload octets
  allowed by `MAX_EXPANDED_PAYLOAD_PER_INPUT_BYTE`, omit the anchor or
  instance that passes them and give it up as `decode/record.undecoded`; recorded
  outcomes and the `MAX_DISCARDED_*` allowances bound the work of repeated
  misses.
  Assembly occurrence expansion stops at `MAX_OCCURRENCES` or at a path of
  `max_recursion_depth` usages and reports a loss. Occurrence paths are
  parent links, so their storage is linear in the occurrences.
- Creo View PVS element child lists charge `enter_nested`, and every PVS
  element charges one entity. Every PVS and OL GZIP stream expands through
  `begin_expand` with no declared size, and its trailer is checked after
  expansion. String-pool references share the pool entry; the text of
  definition names, properties, and occurrence names is charged to
  `retain_bytes` when it enters the model. Occurrence expansion stops at
  `MAX_OCCURRENCES` or at a path of `max_recursion_depth` definitions and
  reports a loss. Occurrences are parent links keyed by expansion index, so
  their storage is linear in the occurrences; definitions and occurrences
  are admitted with `admit_entities`.
- USDA prim bodies, metadata blocks, dictionaries, tuples, lists, time
  samples, and variant sets charge `enter_nested`, and every prim and
  property statement charges one entity. Composition charges
  `enter_nested` for each composed prim and each arc it expands; an arc to
  a prim already on the expansion path is a cycle and reports a loss. Every
  composed prim charges one entity, so internal references that expand a
  subtree many times stop at `max_entities`. Prim names and tessellation
  arrays are charged to `retain_bytes` when they enter the model, and
  occurrences, definitions, and geometry are admitted with
  `admit_entities`.
- IGES occurrence expansion, type-124 chains, and composite flattening
  charge `enter_nested`. The occurrence and type-124 depth limits take `min`
  with `max_recursion_depth`. At a local limit the walk stops and reports a
  loss.
- IGES planar polyline simplicity checks share one `PlanarPairBudget` per
  pass, because one boundary can be named by many faces. Bézier extraction,
  splitting, and degree elevation are checked against `MAX_BEZIER_BLENDS`
  before they run.
- Rhino instance expansion uses a local `ExpansionBudget` and reports a
  warning loss. Mesh buffer inflation charges the decompression ceilings,
  and a document-wide `MeshBudget` fails one mesh without fusing.
- NX geometry completion uses model-wide `GeometryWorkBudget` slices built in
  `decode/build.rs`. Exhaustion reports
  `intersection.pcurve-completion-bounded` or
  `geometry.adaptive-work-bounded`. Source scans report
  `source.scan-bounded`. Object-record identities over
  `MAX_STABLE_GRAPH_WORK` report `native.record-identity-bounded`.
  Display-JT lanes over `MAX_ARITHMETIC_WORK` report
  `container.display-jt-arithmetic-lane-bounded`; the topology lane-count
  search reads past the last lane and counts its probes only when it finds
  no count. A Display-JT polygon mesh that does not resolve is skipped alone
  and reports
  `carrier.display-jt-tessellations-dropped`; this is not a resource bound.
- F3D recipe, pattern, parameter-order, spatial-profile, extrude-profile
  arrangement and densification, and history-binding bounds report losses.
  The local work cap losses name the constant. History binding uses
  `retained_bytes_available()`.
- FreeCAD link-array occurrences are admitted against `max_entities` before
  any is built. Sketch profile chaining has one `WorkBudget` per sketch; a
  sketch that exhausts it keeps no profiles and reports
  `sketch.profile-matching-bounded`, and later sketches still chain.
- Creo relation string text is working storage, not retained bytes: native
  record text is charged when it is serialized. Relation evaluation bounds it
  with local text caps that drop the value and report
  `curve-expression.text-capped`. Interpolation splines, plane-support lane
  searches and section sign branches skip the item at a local cap and report
  a loss naming the bound.
- Inventor measures each parameter expression once per shared node. A
  parameter whose text would pass `MAX_EXPRESSION_TEXT_BYTES` is skipped and
  reported as `parameter.expression-bounded`; emitted text charges
  `retain_bytes`.
- `cadmpeg-codec-sat` has no production
  `MAX_*`.
- CATIA zero-entity topology, object-stream selection, the `7C02` catalog
  scan, planar loop classification, and B5 population attachment report
  bound-specific losses. The standard-family evidence searches (limit-curve
  points, native ports, endpoint pairs, circular-range selection, and
  repeated-edge trim handles) report `topology.standard-search-bounded` at
  the skip. Standard-family topology search exhaustion, including the
  duplicate-face completion, is reported as the topology failure "the
  bounded topology search exhausted its operation or solution budget".
