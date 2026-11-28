# Admissibility routes

## Container metadata recovery

The source-recovery policy is in [architecture.md](architecture.md#source-recovery).
These are the container and header paths reviewed for metadata-only refusal.
This table does not describe every entity-level admission rule.

| Codec | Metadata and interpretation boundary |
| --- | --- |
| STEP | `parse::validate_header` locates records by name. Description, filename, and closed descriptive literals produce diagnostics without discarding DATA. A present malformed entity name keeps its parameter slot; omitted-name recovery requires a known shorter mapping. DATA geometry numeric admission remains strict. A unique readable schema remains required. Nonconforming records retain their source bytes. |
| IGES | `global::Resolution` reports unusable metadata per field; `resolve` applies the existing field-specific fallbacks. Delimiters must resolve before fields can be framed. An unreadable suffix after version field 23 remains source-only without guessing later boundaries; the trustworthy interpretation prefix survives. Directory display fields are admitted independently of identity, Parameter Data ownership, status, and transformations. Unreadable display metadata has no substituted value. |
| Rhino | `container::scan` accepts an absent or short introductory comment. Chunk bounds remain required. The first properties table supplies the writer version before object interpretation, including when it follows the objects. Absent, repeated, or reordered properties tables and misplaced metadata do not discard independent objects. The first properties and settings tables own metadata; other copies remain opaque. |
| FreeCAD | `container::parse_document` diagnoses version aliases and retains the canonical value on conflict. Invalid `FileVersion` remains unverified; only nonempty inline legacy element maps depend on its interpretation. Independent B-rep geometry and self-framed maps survive. The numeric `SchemaVersion` value selects persistence tags, including equivalent spellings such as `02` and `+2`; duplicate persistence sections remain ambiguous. Redundant XML counts, malformed unused links, unrelated missing attachments, and unreadable optional ZIP payloads recover with explicit source-only states and diagnostics. Required shape sidecars remain readable dependencies. Optional naming carriers recover independently with explicit unreadable states and diagnostics. Malformed GUI metadata remains source-only under a valid native GUI document identity; missing or ambiguous cameras do not discard valid appearance providers. |
| Inventor | `property_set::inventory` retains malformed property streams as `PropertySetState::Malformed`. Counted binary kernel product strings advance by byte length even when their text encoding is unreadable. Kernel tolerances are admitted independently with defaults and diagnostics for unusable values. The compound storage and RSeStorage are needed to locate model data. |
| Fusion | `container::scan` uses the manifests to resolve the Design asset folder. Counted descriptive manifest payloads recover with diagnostics. Unreadable preview, image, and OGS cache members retain stored source bytes and diagnostics. Asset-selection names, GUID search anchors, required layout markers, and byte bounds remain required because the selected asset determines which model is decoded. Text SMT uses shared SAT header recovery. Text and binary kernel tolerances are admitted independently, with diagnostics and defaults for unusable values or unit conversion overflow. Valid precision below the analytic floor remains unsupported. |
| SolidWorks | `container` skips unreadable optional expanded payloads while propagating resource failures. Body streams are selected by their stream names and Parasolid headers. |
| CATIA | `container::scan_bytes` returns a fallible container scan. Magic, directory fields, and segment bounds gate payload discovery; optional summary records are collected independently. |
| Creo | `container::scan_bytes` returns a fallible section scan. Header and section framing locate payloads; downstream semantic admission and optional model-name collection have separate boundaries. |
| NX | `container::scan_bytes` validates directory offsets, overlap, and file spans. These locate payloads; they are not descriptive header fields. |
| Creo View | `package::Package::open` admits the archive through `ArchiveSnapshot`. An unreadable central directory recovers the members from their local headers and reports `pvz/container.directory-recovered`. Exactly one `.pvs` member remains required. A structure member that declares no known signature or XML version is read on the residual path as `pvz:unknown` with `pvz/source.dialect-unverified`. A structure member that does not decode is given up as one part (`creo_view_pvz.md` §9), and the package metadata survives. A property stream applies whole or not at all: a stream that does not decode is given up as a `pvs-stream` part, and the other streams survive. A directory stream of an undefined type reports `pvz/structure.stream-undecoded`. Each geometry member opens and decodes independently: a member that does not open or decode is given up as one part, and an exact-geometry stream that does not decode is given up as an `ol-stream` part while the member keeps its tessellation. A resource refusal scoped to one part gives up that member; a session refusal propagates. |
| OpenUSD | `detect::header` reads the text cookie or the USDC bootstrap identifier. A text header that names no declared version is read on the residual path as `usd:unknown` with `usd/source.dialect-unverified`; a USDC layer is identified and refused with `UnsupportedDialect`. The USDA grammar admits the whole layer or refuses it as malformed with its line and column. An attribute value that does not have its declared type, a transform-op stack that does not resolve, and a mesh, curve, or patch whose counts disagree each report a loss and keep the rest of the layer. Resource failures propagate. |
| eDrawings | `package::Package::open` admits the archive through `ArchiveSnapshot`. An unreadable central directory recovers the members from their local headers and reports `edrawings/container.directory-recovered`. An archive without an `eModel` member is refused as `WrongFormat`, and a wrapper that does not read is refused. A stream version outside `19.10`, `18.00`, and `19.00` is read on the residual path as `edrawings:unknown` with `edrawings/source.dialect-unverified`. A zlib stream that does not open is given up as one part with `decode/record.unreadable`, and the package metadata survives. A model stream whose records break a record rule is given up with `decode/record.unreadable`, and one that holds an opcode or a variant that the decoder has no reading for is given up with `decode/record.undecoded`; the package then carries no model data. A shell, instance, or body that does not build reports a loss and keeps the rest of the model. Resource failures propagate. |
| Alibre | `container::AlibreContainer::open` admits the compound file through `CompoundSnapshot` and requires the `SAB_DATA` and `STEP_DATA` streams. A `SAB_DATA` stream without the ACIS binary magic or its record-stream boundary classifies as `alibre:unknown` and is refused with `UnsupportedDialect`. An ACIS stream that does not frame or decode refuses the part. Resource failures propagate. |
| SAT/ASM | `sat::parse_header` recovers malformed newline-framed product metadata, unused record-count declarations, and optional tolerances. Counted binary product strings advance by byte length independently of text encoding. Decode retains exact noncanonical header bytes and reports defaults for unresolved tolerances. Unit scale, record-stream framing, and reference width remain required interpretation fields. |

Successful decode is not a valid IR. Decoder gates use lists of named rules
([`Admission`](../crates/cadmpeg-ir/src/validate/admit.rs)); final document
validation remains `validate_neutral` (+ fidelity + native at the application
boundary). Accept/reject fixtures are in
[admissibility-fixtures.md](admissibility-fixtures.md).

Application validation uses the physical input byte length measured by the
loader to establish input-proportional allowances in its fresh processing
context. Check and conversion keep the configured absolute resource ceilings;
they do not retain another source copy or infer the allowance from metadata.

## Shared core

Every validation finding names its rule (`Finding.rule`, `[<codec>/]<area>.<detail>`).
`DRAFT_CORE` lists, by name, the rules on model-class records that a decoder
may test a candidate reading against (§3.3 of the CADIR specification): the
rules of the `Identity`, `ReferentialIntegrity`, `NativeLinks`,
`CoedgePairing`, `ShellTopology`, `WireTopology`, `CarrierReachability`,
`ParameterDomain`, and `GeometricConsistency` checks outside the design and
process layers, as the validator stated them when the routes became rule
lists. A finding whose rule a route does not list refuses no candidate, so a
rule added to the validator later changes no admission until a route lists it
in `validate/admit.rs`. A finding of the design or process layer refuses no
candidate either, also where it reports a listed rule inside a stored state. `python3 scripts/name-validation-rules.py --admission`
prints the rules of those checks as the validator states them now.

API: `cadmpeg_ir::validate::admit::{admit, admit_with_annotations,
admit_with_additional_native_identities, Admission}` and the route
constants below.

## Per-route subsets

| route                                            | constant / policy                           | notes                                                                                                                                   |
| ------------------------------------------------ | ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| F3D history body                                 | `DRAFT_CORE`                                | one nest and its origin feature in a scratch document; a nest that fails is dropped with `feature.history-body-undecoded`              |
| Rhino draft                                      | `RHINO_DRAFT` = core + annotation rules     | arena order excluded (pre-finalize)                                                                                                     |
| Rhino instance                                   | `RHINO_INSTANCE` = core                     | arena order excluded; no mid-expansion finalize                                                                                         |
| CATIA admission                                  | `CATIA_ADMISSION` = core                    | finalize first; pending native identities via admit helper                                                                              |
| IGES projection check                            | full `validate_neutral`                     | refuses nothing: each Error finding is one `iges/projection.invalid` loss, and the decode returns the document                         |
| SLDPRT written document                          | full `validate_neutral`                     | refuses nothing: each Error finding is a `record.invalid` loss, and the part is written                                                 |

Codecs not listed run no decode/export admission gate; their documents are
checked only at the application boundary (`validate_neutral` + fidelity +
native validators).

## Routes and dialects

No route differs per dialect. A route is selected by the pipeline stage that
reaches it, and every gate above runs the same check set for every dialect of
its format. There is no per-dialect row here because there is no per-dialect
route.

What is dialect-keyed is the evidence each classification rests on. Fixture
coverage is per row in [`dialect-support.toml`](../crates/cadmpeg-registry/docs/dialect-support.toml), and it
is uneven:

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

## Native passthrough arenas

A semantic writer refuses a native arena it cannot preserve, so an arena the
writer does not regenerate is admitted by name. An IGES record that failed
typing is a given-up part (CADIR §3.3): its `unknowns` stub and its retained
bytes hold it. The export wrapper charges no record of an `unknowns` arena,
because the decode already charged the part. The semantic writer declares the
Directory and Parameter cards of the record dropped, as cards of no typed
entity. Byte-exact replay is a separate path: it copies the retained source
image, which holds every quarantined card.

## Application composition

`cadmpeg check` / convert validation =

1. `validate_neutral` (or `validate_neutral_with_source_fidelity`)
2. plus every registered native validator whose namespace is present, with the
   loaded document's optional `SourceFidelity`
3. plus the measurement findings `cadmpeg_measure::findings`: `RingOrder`
   and `Orientation` warnings ([measurement.md](measurement.md#check-findings))

No route above lists `RingOrder` or `Orientation`, and no decoder runs
measurement, so decoding and export admission never refuse on them.

NX native validation admits Deltas structure, then compares all six Deltas
arenas with the projection derived from retained inflated stream bytes. This
comparison requires exact record identities, complete record sets, field
values, effective schemas, custom metadata, headers, prefixes, and residual
spans. A stopped stream reports `NativeLinks` `Info` with its stop offset and
reason. Its bounded prefix was parsed, and its residual range and digest were
compared.

Without a sidecar, each claimed Deltas stream reports structural-only
`NativeLinks` `Info`. A digest-only retained stream reports the same limit.
A present sidecar missing a claimed stream reports `NativeLinks` `Error`.
When any Deltas arena contains records, inline canonical retained Deltas
streams also enter comparison, including a stream whose native records were
deleted. Empty Deltas arenas make no source claim and produce no Deltas source
finding. Resource refusals remain typed check-stage errors.

Encoders count rows with `CadIr::census`, not validation.
