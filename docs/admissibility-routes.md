# Admissibility routes

## Container metadata recovery

The source-recovery policy is in [architecture.md](architecture.md#source-recovery).
These are the container and header paths reviewed for metadata-only refusal.
This table does not describe every entity-level admission rule.

| Codec | Metadata and interpretation boundary |
| --- | --- |
| STEP | `parse::validate_header` locates records by name. Description, filename, and optional header conformance produce diagnostics. A unique readable schema remains required. Nonconforming headers are retained by `reader::decode_exchange_mode`. |
| IGES | `global::Resolution` reports unusable metadata per field; `resolve` applies the existing field-specific fallbacks. Delimiters must resolve before fields can be framed. |
| Rhino | `container::scan` accepts an absent or short introductory comment. Chunk bounds remain required. Table order is still used to obtain the writer version before decoding object records. |
| FreeCAD | `container::parse_document` reports aliased or conflicting producer-version metadata. `SchemaVersion` selects persistence tags; duplicate persistence sections remain ambiguous. |
| Inventor | `property_set::inventory` retains malformed property streams as `PropertySetState::Malformed`. The compound storage and RSeStorage are needed to locate model data. |
| Fusion | `container::scan` uses the manifests to resolve the Design asset folder. Manifest failures cannot be treated as absent producer metadata: the selected asset determines which model is decoded. |
| SolidWorks | `container` skips unreadable optional expanded payloads while propagating resource failures. Body streams are selected by their stream names and Parasolid headers. |
| CATIA | `container::scan_bytes` collects directory and segment facts without a whole-file parse Result. Segment boundaries and downstream model admission are separate from descriptive metadata. |
| Creo | `container::scan_bytes` collects framed sections without a whole-file parse Result. The section scan and semantic admission remain separate. |
| NX | `container::scan_bytes` validates directory offsets, overlap, and file spans. These locate payloads; they are not descriptive header fields. |
| SAT/ASM | `decode::build_result` reports invalid header tolerances and retains defaults. Record-stream location and reference width are interpretation fields. |

Successful decode is not a valid IR. Decoder and export gates use documented
subsets of [`Check`](../crates/cadmpeg-ir/src/report.rs); final document
validation remains `validate_neutral` (+ fidelity + native at the application
boundary). Accept/reject fixtures are in
[admissibility-fixtures.md](admissibility-fixtures.md).

## Shared core

`DRAFT_CORE_CHECKS` =

- `Identity`
- `ReferentialIntegrity`
- `NativeLinks`
- `LoopClosure`
- `CoedgePairing`
- `ShellTopology`
- `WireTopology`
- `CarrierReachability`
- `ParameterDomain`
- `Bounds`
- `GeometricConsistency`

API: `cadmpeg_ir::validate::admit::{admit, admit_with_annotations,
admit_with_additional_native_identities, filter_checks}` and the route
constants below.

## Per-route subsets

| route                                            | constant / policy                           | notes                                                                                                                                   |
| ------------------------------------------------ | ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| Rhino draft                                      | `RHINO_DRAFT_CHECKS` = core + `Annotations` | `ArenaOrder` excluded (pre-finalize)                                                                                                    |
| Rhino instance                                   | `RHINO_INSTANCE_CHECKS` = core              | `ArenaOrder` excluded; no mid-expansion finalize                                                                                        |
| CATIA admission                                  | `CATIA_ADMISSION_CHECKS` = core             | finalize first; pending native identities via admit helper                                                                              |
| IGES rejection                                   | full `validate_neutral`                     | classified keep-full: `DRAFT_CORE_CHECKS` ≡ full errors on all 78 IGES golden fixtures, every one of them `iges:5.3-fixed-ascii`        |
| SLDPRT export precondition (`writer` input)      | full `validate_neutral`                     | keep-full: refusal depends on non-core Checks (`Counts`, …); `SLDPRT_EXPORT_PRECONDITION_CHECKS` is the documented draft/topology floor |
| SLDPRT writer postcondition (after bake/prepare) | full `validate_neutral`                     | postcondition on the writer's own transform; not replaced                                                                               |

Codecs not listed run no decode/export admission gate; their documents are
checked only at the application boundary (`validate_neutral` + fidelity +
native validators).

## Routes and dialects

No route differs per dialect. A route is selected by the pipeline stage that
reaches it — draft candidate, mid-expansion instance, admission after
finalize, export precondition, writer postcondition — and every one of the six
gates above runs the same check set for every dialect of its format. There is
no per-dialect row here because there is no per-dialect route.

What is dialect-keyed is the evidence each classification rests on. Fixture
coverage is per row in [`dialect-support.toml`](dialect-support.toml), and it
is uneven:

- **IGES rejection.** All 78 golden fixtures are `iges:5.3-fixed-ascii`. The
  keep-full classification is therefore a measurement of one row out of
  twenty-two. The other twenty-one carry no fixture, so nothing has ever
  compared `DRAFT_CORE_CHECKS` against full validation on a compressed-ASCII,
  binary, or pre-5.3 document.
- **Rhino draft and instance.** Fixtures sit on `rhino:archive-3`, `-4`,
  `-50`, `-60`, `-70`, and `-80`; `rhino:archive-1`'s single fixture is a
  reject case (`reject_v1.3dm`), so it exercises detection, not the gates.
  `rhino:archive-2`, `rhino:archive-5`, and `rhino:archive-90` have none.
  `rhino:archive-5` is admitted under its declared four-byte chunk grammar.
  `rhino:unknown` runs the full chunked route under
`Admission::Residual`, so the gates apply to it, but no fixture
  measures either route.
- **CATIA admission.** Every declared `catia:` row carries a fixture, so this
  is the one route measured across its format's whole declared space.
- **SLDPRT export precondition and postcondition.** All twenty fixtures land
  on `sldprt:unknown`. Neither `sldprt:sw-version-pre-12000` nor
  `sldprt:sw-version-12000-plus` has one, so the keep-full classification has
  never been measured against a document that classified onto a declared row.

Narrowing a keep-full route to a `Check` subset is a claim about the documents
that route sees. Read the row above first: on IGES and SLDPRT the current
evidence covers one row each.

## Native passthrough arenas

A semantic writer refuses a native arena it cannot preserve, so an arena the
writer does not regenerate is admitted by name. The IGES semantic writer admits
`quarantined_directory_records` and `quarantined_parameter_records` as
passthrough arenas. It regenerates neither, because a record that failed typing
has no fields to write, and a non-empty one charges
`iges/writer.passthrough-omitted` naming the arena and its record count.
Byte-exact replay is a separate path and is unaffected: it copies the retained
source image, which holds every quarantined card.

## Application composition

`cadmpeg check` / convert validation =

1. `validate_neutral` (or `validate_neutral_with_source_fidelity`)
2. plus every registered native validator whose namespace is present

Encoders count rows with `CadIr::census` / `entity_census`, not validation.
