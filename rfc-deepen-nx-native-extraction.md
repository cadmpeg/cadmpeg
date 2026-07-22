# RFC: Deepen the NX native record extraction tier

## Problem

`cadmpeg-codec-nx` has one real entry point (`NxCodec` implementing `Codec`), but internally the crate's two largest modules form a shallow, tightly coupled pair:

- `native.rs` (15k lines) is a flat catalogue, not a module: ~176 public functions shaped `fn x(&container/&streams) -> Vec<XRecord>` plus ~184 record structs, mixing three unrelated domains (JT display models, Parasolid records, NX feature/OM records). Its interface is as wide as its implementation — the definition of a shallow module.
- `decode.rs` is its only consumer, with 366 references. ~130 extractors are each called exactly once inside `attach_native_object_model` (~2,400 lines), whose first ~600 lines are pure `let x = native::y(...)` wiring in a hand-ordered dependency DAG, followed by a ~135-line all-empty guard and ~1,800 lines of attachment. A 40-field `FeatureOperationSources` borrow-struct exists solely to smuggle record vectors into `attach_feature_operations` (1,634 lines).

Measured structure of the attachment code: ~85% is mechanical per-record boilerplate — a (tag, exactness, annotation-stream, arena-name) tuple per record type, emitted as note/tag/exactness loops and `set_arena` calls — plus four semantic islands (tessellations, source attributes, feature operations, unknown records).

Specific integration risks in the seams:

- **Struct twins.** `native.rs` re-declares ~6 serde twins of `intersection.rs`/`topology.rs` types (`TermUse` ↔ `ParasolidTermUseRecord`, `ChartSourceRecord` ↔ `ParasolidChartRecord`, `OffsetSurface` ↔ `ParasolidOffsetSurfaceRecord`, …) with hand-maintained field copies and 1:1 enum remaps. Two vocabularies for one concept; every schema change is a two-site edit.
- **Double parse, two byte views.** `topology::{offset_surfaces, blend_surfaces, trimmed_curves, surface_curves}` and `intersection::scan` each run twice per Parasolid stream: once on `stream.inflated` (record path) and once on the delta-extended semantic-stream bytes (IR geometry path). Additionally, `feature_operation_labels` and `segment_body_bindings` are computed twice within `decode.rs`, and datum extractors internally recompute `feature_input_blocks`.
- **Copy-paste families.** The 8 `parasolid_*_records` functions share an identical skeleton (stream loop, `is_parasolid()` guard, scanner call, `format!("nx:s{ordinal}:…#{xmt}")` id, sort). Six `feature_datum_csys_*`/`feature_datum_plane_*` function pairs differ only in a type name and one field.
- **Drift-prone conventions.** Arena names, tag strings, and exactness levels live 2,000+ lines from the extractors they describe; the invariant "every extracted record is noted, tagged, exactness-marked, and serialized" is enforced only by hand-maintained parallel blocks.
- **White-box tests lock it all in place.** Of 398 tests, only ~99 exercise the codec boundary; the rest import 50+ private parse functions and assert on `pos`/`offset` fields and hard byte offsets (60× `windows(4).position(...)` offset-hunting). Behavior-preserving refactors break large numbers of tests despite identical output.

Nothing outside the crate imports `native` (verified); the entire catalogue can leave the public surface.

## Proposed Interface

Hybrid of the minimal-entry-point and caller-optimized designs: an eager, compile-checked extraction model; a two-view shared-parse substrate; attachment absorbed behind a declarative const-slice catalogue; generic skeletons for the copy-paste families. No runtime registry, no `TypeId`/`Any`, no macro.

`src/native.rs` becomes `src/native/`:

```
src/native/
  mod.rs        // the cross-module boundary: attach_annotations + re-exports
  substrate.rs  // ParsedStreams — every expensive stream parse, once per byte view (IR-free)
  display_jt.rs // JT display extractors + records            (IR-free)
  parasolid.rs  // parasolid extractors + records + generics  (IR-free)
  segments.rs   // segment index / stream links               (IR-free)
  features.rs   // feature extractors + records               (IR-free)
  om.rs         // om/data-block/expression/extref records    (IR-free)
  model.rs      // NativeModel + eager extraction DAG         (IR-free)
  catalogue.rs  // const CATALOGUE: &[CatalogueRow] — tag/arena/exactness/stream per record
  attach.rs     // the ONLY module allowed to import cadmpeg_ir types for writing
```

```rust
// substrate.rs — single-parse invariant, honest about the two byte views
pub(crate) struct ParsedStreams { pub per_stream: Vec<StreamParses> }
pub(crate) struct StreamParses {
    pub raw: StreamView,       // parses of stream.inflated (record path)
    pub semantic: StreamView,  // parses of delta-extended bytes (IR geometry path);
                               // shares raw's results when the bytes are identical
}
pub(crate) struct StreamView {
    pub graph: topology::Graph,
    pub offset_surfaces: Vec<topology::OffsetSurface>,
    pub blend_surfaces:  Vec<topology::BlendSurface>,
    pub trimmed_curves:  Vec<topology::TrimmedCurve>,
    pub surface_curves:  Vec<topology::SurfaceCurve>,
    pub intersections:   intersection::ScanResult,
}
impl ParsedStreams { pub(crate) fn parse(scan: &Scan) -> Self; }

// model.rs — eager, infallible, best-effort
pub(crate) struct NativeModel {
    pub display_jt: DisplayJtRecords,
    pub parasolid:  ParasolidRecords,
    pub segments:   SegmentRecords,
    pub features:   FeatureRecords,   // replaces FeatureOperationSources, owned
    pub om:         OmRecords,
}
impl NativeModel {
    /// Runs the full extraction DAG in topological order. Malformed data is
    /// omitted, never an error. Each derived family computed exactly once.
    pub(crate) fn extract(scan: &Scan, parsed: &ParsedStreams) -> Self;
    /// Replaces the hand-written all-empty conjunction; derived from the catalogue.
    pub(crate) fn is_empty(&self) -> bool;
}

// mod.rs — the one call decode.rs makes
pub(crate) fn attach_annotations(
    ir: &mut CadIr, scan: &Scan, parsed: &ParsedStreams,
    annotations: &mut AnnotationBuilder, unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), NativeConvertError>;
```

The mechanical attachment strata are driven by a plain const slice — no macro, grep-able:

```rust
// catalogue.rs
pub(crate) struct CatalogueRow {
    pub arena: &'static str,
    pub tag: Option<&'static str>,
    pub exactness: Exactness,
    pub stream: StreamSource,             // Container | PerStream
    pub emit: fn(&NativeModel, &mut NativeNamespace) -> Result<(), NativeConvertError>,
    pub note: fn(&NativeModel, &mut AnnotationBuilder),
    pub len:  fn(&NativeModel) -> usize,  // feeds is_empty and inspect counts
}
pub(crate) const CATALOGUE: &[CatalogueRow] = &[ /* one row per record family,
    in today's emission order — annotation note order is observable output */ ];
```

Records with irregular note shapes (nested toc entries, `derived(...)` fields) get an explicit custom `note` fn — no forcing into the common shape.

The copy-paste families collapse via compile-time generics (no object safety, no erasure):

```rust
// parasolid.rs
pub(crate) trait ParasolidRecords {
    type Row;
    type Record: Serialize;
    const ARENA: &'static str;
    const ID_STEM: &'static str;         // "offset-surface-record"
    fn rows(view: &StreamView) -> &[Self::Row];
    fn xmt(row: &Self::Row) -> u32;
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record;
}
pub(crate) fn per_parasolid_stream<P: ParasolidRecords>(
    scan: &Scan, parsed: &ParsedStreams,
) -> Vec<P::Record>;  // owns the stream loop, guard, id format, sort — once
```

The datum csys/plane pairs get the same treatment: one shared generic body parameterized by a noun-and-hook trait; each pair member becomes ~10 lines.

**Usage — everything that remains in `decode.rs`:**

```rust
let parsed = native::ParsedStreams::parse(scan);
// IR geometry path reads parsed.per_stream[i].semantic.* instead of re-scanning
native::attach_annotations(&mut ir, scan, &parsed, &mut annotations, &mut unknowns)?;
```

**What this hides:** the ~130-binding dependency DAG (ordering owned by `extract`, uncallable wrong); the arena/tag/exactness bookkeeping (one row per family, drift impossible by construction); the stream-loop/id/sort skeletons; the emptiness quirks; which byte view each consumer reads. `decode.rs` sheds ~4,400 lines and all 366 `native::` references.

**Struct twins:** twins whose fields match the scanner type verbatim are deleted — the `topology.rs`/`intersection.rs` type derives `Serialize` (with `#[serde(rename)]` where needed) and serializes directly. Twins that add `id`/`stream_ordinal`/`inflated_offset` or rename fields are the frozen wire schema and survive, but each is confined to one `record()` conversion colocated with its family, not a re-declaration 12k lines away. The twins' second parse disappears in both cases.

## Dependency Strategy

**In-process — merged directly.** All dependencies (`om`, `jt`, `jt_topology`, `parasolid`, `topology`, `intersection`, `container`) are pure computation within the crate. No ports, adapters, or stand-ins.

- `attach_native_object_model`, `attach_feature_operations`, the parasolid-attribute attachers, tessellation inputs, `records_by_operation`, and `preceding_operation_dependency` move from `decode.rs` into `native/attach.rs`.
- Semantic-stream preparation moves into `native/substrate.rs`.
- `topology.rs`, `intersection.rs`, `parasolid.rs`, `om.rs`, `container.rs`, `jt.rs`, `jt_topology.rs` are unchanged; the four topology scanners and `intersection::scan` become substrate-internal call sites.
- `lib.rs`: `pub mod native` → `pub(crate) mod native` (no external consumers exist). `summarize()` routes its `Graph::parse` through the substrate and can use catalogue `len` fns for counts; its attribute strings stay hand-written (observable output).
- IR-write firewall: only `attach.rs` and `mod.rs` may import `cadmpeg_ir` mutation surfaces. The substrate, model, and five domain files stay IR-free and independently testable.

## Testing Strategy

**New boundary tests to write:**

- Golden serialized-output snapshots: for each existing `.prt` builder fixture, decode via `NxCodec` and snapshot the full serialized IR (annotations + `nx` namespace arenas). Written **before any production code moves** — they are the byte-identity oracle for every phase.
- Per-domain extraction tests at the `NativeModel` boundary: bytes in → grouped record vectors out, asserting record content, not parse-cursor positions.
- Substrate tests: a delta-bearing stream fixture asserting `raw` and `semantic` views diverge exactly where the deltas dictate, and share parses where bytes are identical.
- A catalogue coverage test: every record family appears in `CATALOGUE` exactly once; every arena name is unique.

**Old tests to delete (replace, don't layer):**

- The ~300 white-box tests importing private extractors and asserting `pos`/`offset`/`inflated_offset` internals, migrated or deleted per phase as their subject moves behind the boundary. Assertions on decoded record *content* migrate to `NativeModel`-boundary tests; assertions on parse-cursor bookkeeping are deleted.
- The ~25 near-identical `*_partition_stream`/`deltas_*` fixture builders collapse to one or two parametric helpers keyed by (type code, value array); the 20 copies of the deltas preamble literal become one constant.
- `deltas::points` and `deltas::Point` (zero production callers) are removed outright.

**Test environment needs:** none — pure in-process, existing fixture builders suffice.

## Implementation Recommendations

Responsibilities, durable across file moves:

- **The native tier owns**: the record catalogue (what families exist, their wire schemas, their ids, their sort orders), the extraction dependency DAG, the single-parse substrate, and the mechanical attachment of records to annotations and namespace arenas.
- **It hides**: individual extractor functions, the stream-iteration/guard/id/sort skeletons, the raw-vs-semantic byte-view distinction, the emptiness rules, and the twin-type conversions.
- **It exposes**: `ParsedStreams::parse`, `attach_annotations`, and (for the IR geometry path) read access to the parsed stream views. Nothing else crosses the boundary.
- **The decode tier owns**: pipeline orchestration (scan → geometry-vs-metadata branch → IR assembly) and all geometry IR construction. It knows that native records exist; it no longer knows what they are.

Invariants to preserve, in priority order:

1. Serialized output is byte-identical: field names, id formats, sort orders, arena names, annotation note order, absent-key vs empty-vec distinctions.
2. Best-effort semantics: extraction is infallible; malformed data is omitted.
3. Single parse per byte view — but the raw/semantic distinction is load-bearing; conflating the views silently changes output on delta-bearing files.
4. Extractor purity: memo/dedup of derived families is safe only because extractors are pure; state it as a contract on the extraction tier.

Migration order — every step lands green and golden-diffs clean:

0. Write the golden serialized-output snapshots against current behavior.
1. Split `native.rs` into the directory tree with re-exports; zero logic change.
2. Introduce `NativeModel::extract` + `is_empty`; delete the wiring block and guard from the decode tier; delete `FeatureOperationSources`.
3. Move the attachment strata behind the catalogue in `attach.rs`; golden-diff after each batch of rows.
4. Introduce `ParsedStreams`; repoint both consumers; verify the delta-bearing fixtures specifically.
5. Collapse the copy-paste families onto the generic skeletons; delete verbatim twins by deriving `Serialize` on the source types.
6. Migrate or delete the white-box tests whose subjects moved; collapse the fixture-builder duplication.

Known costs, accepted: `NativeModel` is a wide struct (inherent — the consumer genuinely needs per-family access); eager extraction raises the memory high-water mark during geometry construction; `attach_feature_operations` relocates but does not shrink (its 1,600 lines are semantic, not mechanical); the intersection path keeps two scan variants (`scan` vs `scan_with_auxiliary_replacements`) because unifying them would change output.
