# RFC: Restructure cadmpeg-codec-catia

## Problem

The crate is ~54k lines across 15 flat files. File-level layering is sound (geometry ⟂ topology, both consumed by decode; b5 parse → b5_transfer emit is a maintained seam; `object_graph.rs` and `container.rs` are clean). The friction is intra-file:

- No shared primitives layer. Vector math (`cross`/`unit`/`dot`/`scale`) exists in 4–5 copies across `b5.rs`, `b5_transfer.rs`, `geometry.rs`, `zero_entity.rs`, `decode.rs`. Compact-int and reference-token readers are duplicated verbatim between `b5.rs` and `e5.rs`. The 2-byte-tag record scan loop is reinvented per decode family. Union-find is hand-rolled twice. The guard `is_finite() && > 0.0 && < 1e6` appears ~97 times.
- `geometry.rs` (5.5k lines) is six per-family decoder groups (`standard`, `e5`, `b2`, `a5a8`, `consolidated`, `zero_entity`) flattened into one namespace. The cylinder/cone/torus analytic decoders are stamped out three times differing only in offsets and endianness.
- `topology.rs` (15.8k lines) is three independent combinatorial solvers (incidence backtracking, mesh missing-edge enumeration, a ~2,300-line `MeshQuotient` CSP), a byte-parsing layer, and a container type concatenated with 4k lines of inline tests. ~40 `pub fn` for a single consumer. Eight `fn`/`fn_with_budget` one-line twin pairs; nested `walk`/`search` helpers re-declared up to 7×.
- `decode.rs` (9k lines) is four copy-pasted variant pipelines (`standard`, `zero_entity`, `e5`, `freeform`) all shaped `try_decode → transfer topology → attach geometry`, returning the same `ProjectedDecode` tuple alias. Fallback order is implicit in an if-chain.
- `b5_transfer.rs` has an 851-line `transfer_complete` whose single scope exists only to share fourteen id-map locals; `attach_standard_topology` (811 lines) and `transfer_e5_topology` (677 lines) repeat the shape.
- `native.rs` mirrors `geometry::Consolidated*` as `CatiaConsolidated*` field-for-field (~350 lines), adding only id/offset decoration.
- `lib.rs` marks `b5`, `e5`, `geometry`, `topology`, `zero_entity`, `container`, `catalog`, `value_block`, `variant` fully `pub` for "format-level access". The only external consumers in the workspace are `CatiaCodec` (registry) and eight fuzz targets that discard parser output.

## Proposed Interface

Public surface shrinks to one entry point plus a fuzz facade:

```rust
pub struct CatiaCodec;                 // impl cadmpeg_ir::codec::Codec — unchanged contract

#[cfg(feature = "fuzz")]
#[doc(hidden)]
pub mod fuzz {                          // ()-returning wrappers for cadmpeg-fuzz targets
    pub fn container_directory(data: &[u8]);
    pub fn b5_parse(data: &[u8]);
    pub fn e5_topology(data: &[u8]);
    pub fn zero_entity_parse(data: &[u8]);
    pub fn geometry_vertices(data: &[u8]);
    pub fn geometry_surface_prefixes(data: &[u8]);
    pub fn geometry_a5_surfaces(data: &[u8]);
    pub fn geometry_a8_surfaces(data: &[u8]);
}
```

Internal module tree as landed (all `pub(crate)` at most):

```
src/
  lib.rs            CatiaCodec + Codec impl; cfg(feature = "fuzz") facade (fuzz.rs)
  decode.rs         orchestrator (109 lines): container scan → ordered route table → fallback
  assemble.rs       shared emit scaffolding as free functions: annotate, source_meta,
                    preserve_raw_payload, admissibility, report/metadata builders, cgm_source,
                    cross-family curve/angle helpers
  wire/
    cursor.rs       Cursor: finite-checked f64 (f64_raw escape), point3/vector3/unit3/skip,
                    compact_uint, object_ref (extended/restricted reference dialects)
    tokens.rs       (&[u8], &mut usize)-signature adapters over Cursor for the b5/e5 scan loops
    bytes.rs        absolute-offset readers (f64_le, f64_point, compact_int, persistent_ref, …)
    records.rs      A/B record framing (ConsolidatedRecord/Frame, a_/b_family_frames),
                    jet-pcurve byte vocabulary, vertex roster scanner
  analytic.rs       named canonical frame readers over Cursor: cylinder_uvr, cone_ozra,
                    torus_ozrr; a fourth field order is a fourth named fn, not a DSL
  nurbs.rs          cross-family NURBS, B-spline, and analytic-curve math
  solve/            combinatorial solvers plus the byte-table readers that feed them
    union_find.rs  matching.rs
    incidence.rs    incidence backtracking search
    missing_edge.rs mesh missing-edge enumeration
    mesh_quotient.rs the CSP; one file — its propagate/search state does not separate
  families/
    mod.rs          FamilyOutput; the ordered route table — order is fallback behavior,
                    documented as an invariant
    standard/       records.rs, topology.rs (StandardTopology container + orientation),
                    fbb.rs (byte layer), decode.rs (route), tests.rs
    b5/             graph.rs (parse + graph resolution, fused by shared record types),
                    transfer/{mod,surfaces,pcurves,vertices,edges,faces}.rs, vecmath.rs, tests.rs
    e5/             records.rs, graph.rs (E5Topology pipeline), decode.rs (route), tests.rs
    b2/  a5a8/  consolidated/    records.rs + tests.rs (record vocabularies, no route)
    zero_entity/    records.rs, graph.rs, decode.rs (route), tests.rs
    freeform/       mod.rs (route: b5 object-stream transfer first, then a5a8 +
                    consolidated surface pools)
  container.rs  catalog.rs  value_block.rs  variant.rs  object_graph.rs   (unchanged)
  native.rs         serialization decoration over family record types; the CatiaConsolidated*
                    mirror is reduced, not deleted — most mirror types rename pos to
                    byte_offset and add id/family, so they pin the namespace payload
  tests.rs          codec-contract behavior tests + shared fixture builders
```

Family record modules are bespoke free-function scanners over `wire` readers; the `CLASSES` dispatch-table template did not land — the deviation is uniform across families, so it is the settled shape. The dominant change — decoding one more record class — is one parser fn plus its scan-loop arm in one family file, plus a fixture test.

Routes are plain structs (`applicable: fn`, `decode: fn`; no id field — nothing reads one) in one ordered slice. Only the four real pipelines are routes; b2/a5a8/consolidated are record vocabularies consumed by routes. No stage traits: the two-axis distinction (record family vs decode route) is the honest shape, and trait plumbing over it is ceremony.

b5 and e5 share `wire/` and `analytic.rs` but keep separate graph and emit types. Their structural parallelism is coincidental in a reverse-engineered format; merging would encode it as a constraint.

## Dependency Strategy

All in-process, no re-export shims. `wire` → `cadmpeg_ir` only; `analytic`/`nurbs` → `wire`; `assemble` → `cadmpeg_ir`; `decode` → `families` + `container` + `assemble`. The family use-edge graph is acyclic: b2 → a5a8; consolidated → a5a8 + b2; standard → freeform; e5/a5a8 have no outgoing family edges. General vector ops (dot, cross, scale, unit, distance) live in `cadmpeg_ir::math`; `families/b5/vecmath.rs` keeps the b5 `[f64;3]` helpers whose two `unit` normalisations diverge at the bit level (reciprocal-multiply vs division). No promotion of `wire::Cursor` to a shared crate until a second codec demonstrates the same shapes.

Layering exceptions that hold in the landed state, recorded as debt below: `solve/missing_edge` and `solve/mesh_quotient` import standard-family types and byte parsers (the solvers are not byte-free); freeform's route runs the whole b5 transfer before its surface-pool path; consolidated and b5 reference each other through inline paths.

## Recorded Debt

- solve ↔ families/standard inversion: extracting real Problem/Solution boundary types across ~9.5k solver lines is its own design phase, not a review side-effect.
- freeform → b5 whole-pipeline edge: freeform is two fused routes (b5 object-stream, then surface pools); splitting into two ROUTES entries would make the table honest but changes fallback interleaving and needs care.
- standard → freeform edge (`append_freeform_surface_pools`): the appender belongs with the pool vocabulary, not in either route.
- b5 ↔ consolidated inline-path cycle (`object_stream_vertices` / `framed_ranges`).
- native mirror completion: `CatiaConsolidatedAnalyticCircleDescriptor` adds only the `pos`→`byte_offset` rename; the serde-derive-on-family-type pattern proven for `ConsolidatedEdgeDefinitionData` extends to it and the pcurve mirror, one type at a time with serialization-equality proof each.
- `wire/records.rs` keeps `Consolidated*` names for framing types that are no longer consolidated-specific; renaming cascades ~40 sites.
- Root `tests.rs` still holds internal-API tests beside the codec-contract tests; family-specific stragglers can continue migrating.
- `#[cfg(test)]` fields inside production structs (test-only parse outputs) mean the tested artifact differs from the shipped one; the solve byte-parsing test-only entry points are the wrong layer regardless of the idiom.
- `wire/tokens.rs` legacy `(&[u8], &mut usize)` adapter surface persists while the b5/e5 scan loops read through it; migrating ~50 call sites to `Cursor` is churn with no behavior gain until those loops are next reworked.

## Testing Strategy

- **Frozen net**: the ~90 behavior tests through `CatiaCodec`/`CatiaNative::decode` pass unmodified at every phase boundary.
- **New boundary tests**: per-family fixture tests in `families/*/tests.rs` through `parse_all`/family decode; direct solver tests in `solve/` against constructed problems (first time the solvers are testable without bytes).
- **Old tests to delete**: the ~120 tests calling individual `pub fn` decoders and asserting internal struct fields, replaced as their family migrates. Diff per-family test counts before/after to catch silent coverage narrowing.
- **Behavioral landmines to watch**: finite-by-default `Cursor` reads (sites that intentionally tolerate non-finite bytes must use `f64_raw`); scan-loop resync differences (use `LengthRule::Custom`, or keep a family-local loop if it genuinely differs); route-table ordering.

## Implementation Recommendations

All phases below are landed (commits `ae2dbd8d..815d5e06`): separate additive commits, behavior suite green at every boundary, callers importing from owning modules from the first commit. Deviations from the plan are reflected in the module tree above and in Recorded Debt.

- **Phase 0 — baseline.** Commit the in-flight extrusion-carrier work on a green suite so refactor commits are purely structural.
- **Phase 1 — primitives.** Create `wire/` (cursor, scan, tokens) and `solve/union_find.rs`; unify the b5/e5 compact-int and reference readers; move general vector ops to `cadmpeg_ir::math` and delete all local copies; add `finite`-guard helpers. Wide but mechanical.
- **Phase 2 — analytic.** Named frame readers; collapse the triplicated cylinder/cone/torus decoders (`zero_entity_*`, `e5_*`, `decode_curved` arms) onto them.
- **Phase 3 — geometry split.** `geometry.rs` → `families/{standard,e5,b2,a5a8,consolidated,zero_entity}/records.rs` with the table template; shared byte helpers dissolve into `wire/`.
- **Phase 4 — topology split.** Solvers → `solve/{incidence,missing_edge,mesh_quotient}.rs` with named boundary types; byte parsing → `families/standard/records.rs`; `StandardTopology` → `families/standard/topology.rs`; collapse the `_with_budget`/`_impl` twins; delete test-only production duplicates.
- **Phase 5 — routes.** `decode.rs` → orchestrator + ordered route table; four pipelines move into their family `mod.rs`; `ProjectedDecode` → `FamilyOutput`; shared emit scaffold → `assemble.rs`; decompose `attach_standard_topology` and `transfer_e5_topology`/`transfer_zero_entity_topology` into staged passes.
- **Phase 6 — b5 transfer.** `transfer_complete` → `families/b5/transfer/` staged passes over an explicit id-table struct; per-variant surface lowering colocated with parsers in `records.rs`.
- **Phase 7 — native.** Delete the `CatiaConsolidated*` mirror; `native.rs` consumes family record types directly, adding only id/offset decoration.
- **Phase 8 — surface shrink.** `lib.rs` → `CatiaCodec` + fuzz facade; edit the eight fuzz targets and fuzz crate Cargo.toml atomically; redistribute remaining `tests.rs` internal tests into `families/*/tests.rs`; rewrite lib.rs docs to point at `docs/formats/`.
- **Phase 9 — post-refactor review.** Re-read every touched file top to bottom as a fresh review; collapse any new shape that emerged; verify no re-export shims, no orphaned helpers, template consistency across families.
