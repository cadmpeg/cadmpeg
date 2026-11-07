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
| FreeCAD | `container::parse_document` diagnoses version aliases and retains the canonical value on conflict. Invalid `FileVersion` remains unverified; only nonempty inline legacy element maps depend on its interpretation. Independent B-rep geometry and self-framed maps survive. The numeric `SchemaVersion` value selects persistence tags, including equivalent spellings such as `02` and `+2`; duplicate persistence sections remain ambiguous. Unreadable XML elements and ZIP members are given-up parts. Independent shape and presentation records remain available. |
| Inventor | Unreadable property streams are given-up parts. Counted binary kernel product strings advance by byte length even when their text encoding is unreadable. Kernel tolerances are admitted independently with defaults and diagnostics for unusable values. The compound storage and RSeStorage are needed to locate model data. |
| Fusion | `container::scan` uses the manifests to resolve the Design asset folder. Counted descriptive manifest payloads recover with diagnostics. Unreadable preview, image, and OGS cache members are given-up parts. Asset-selection names, GUID search anchors, required layout markers, and byte bounds remain required because the selected asset determines which model is decoded. Text SMT uses shared SAT header recovery. Text and binary kernel tolerances are admitted independently, with diagnostics and defaults for unusable values or unit conversion overflow. Valid precision below the analytic floor remains unsupported. |
| SolidWorks | `container` skips unreadable optional expanded payloads while propagating resource failures. Body streams are selected by their stream names and Parasolid headers. |
| CATIA | `container::scan_bytes` returns a fallible container scan. Magic, directory fields, and segment bounds gate payload discovery; optional summary records are collected independently. |
| Creo | `container::scan_bytes` returns a fallible section scan. Header and section framing locate payloads; downstream semantic admission and optional model-name collection have separate boundaries. |
| NX | `container::scan_bytes` validates directory offsets, overlap, and file spans. These locate payloads; they are not descriptive header fields. |
| Creo View | `package::Package::open` admits the archive through `ArchiveSnapshot`. An unreadable central directory recovers the members from their local headers and reports `pvz/container.directory-recovered`. Exactly one `.pvs` member remains required. A structure member that declares no known signature or XML version is read on the residual path as `pvz:unknown` with `pvz/source.dialect-unverified`. A structure member that does not decode reports `pvz/structure.member-malformed` and keeps the package metadata. A property stream applies whole or not at all: a stream that does not decode reports `pvz/structure.property-stream-malformed`, and the other streams survive. A directory stream of an undefined type reports `pvz/structure.stream-undecoded`. Each geometry member opens and decodes independently: a member that does not open or decode reports `pvz/geometry.member-malformed`, and an exact-geometry stream that does not decode reports `pvz/geometry.exact-undecoded` and keeps the member's tessellation. Resource failures propagate. |
| OpenUSD | `detect::header` reads the text cookie or the USDC bootstrap identifier. A text header that names no declared version is read on the residual path as `usd:unknown` with `usd/source.dialect-unverified`; a USDC layer is identified and refused with `UnsupportedDialect`. The USDA grammar admits the whole layer or refuses it as malformed with its line and column. An attribute value that does not have its declared type, a transform-op stack that does not resolve, and a mesh, curve, or patch whose counts disagree each report a loss and keep the rest of the layer. Resource failures propagate. |
| SAT/ASM | `sat::parse_header` recovers malformed newline-framed product metadata, unused record-count declarations, and optional tolerances. Counted binary product strings advance by byte length independently of text encoding. Decode retains exact noncanonical header bytes and reports defaults for unresolved tolerances. Unit scale, record-stream framing, and reference width remain required interpretation fields. |

## Validation and application composition

A decoder gives up unreadable parts and reports `record.unreadable`. A
validation finding does not refuse the decode. The document keeps every
independent readable record; records that need a dropped record are dropped
transitively. The [document contract](document-contract.md#partial-decode)
states the part boundary and whole-file refusal rules.

`cadmpeg check` and conversion run `validate_neutral`, the registered native
validators for namespaces present in the document, and the measurement
findings `RingOrder` and `Orientation` ([measurement.md](measurement.md#check-findings)).
The loader supplies its measured input byte length to the processing context for
input-proportional allowances. Conversion reports findings and writes the
available content. `--reject-errors` applies the caller's refusal policy.
