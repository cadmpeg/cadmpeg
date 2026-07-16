<!-- SPDX-License-Identifier: Apache-2.0 -->
# Rhino decoder calibration notes (Phase 2 freeze input)

Per-module calibration observations for the doc section 5.2 alloc/work/depth
freeze. This file records the measurement basis for the Rhino codec so the
freeze step calibrates against real charge profiles, not noise. Numbers here
are provisional until the freeze; each row states its fixture basis.

## Status

`wire.rs` (pure fixed-size primitives, no budget charge), `cage.rs` (the NURBS
cage control net), `polyedge.rs` (the persistent polyedge-reference
construction), and `hatch.rs` (the boundary-loop table) have graduated to
`migrated` on this branch. `cage.rs` reads its
record body through a `View` on a codec-local `DecodeContext`, sizing the knot
vectors and control net with `View::counted` proofs and `exact_vec`, reserving
derived weights through `alloc_unfloored` under the `MAX_CONTROL_POINTS` cap,
and charging `work` per count-framed loop. `polyedge.rs` reads the record body
and each child segment through a `View` on a codec-local `DecodeContext`, sizing
the parameter table and segment list with `View::counted` proofs and
`exact_vec`, committing scalars through the `req_*` mirror, and charging `work`
per parameter read plus a fixed field count per segment; child-record framing is
located by `chunk_at`/`parse_class_wrapper` typed ranges (no `View::window`
egress). The remaining decoder modules are `legacy` in
`parser-manifest.toml` with per-module reasons: they read object-record bodies
through the crate-local `BoundedReader` over `&[u8]` and size collections with
`Vec::with_capacity`, so their alloc/work charges are not yet driven through
`ctx.exact_vec`/`read_counted`/`ctx.descend`. The one platform charge already
live is `decompressed_bytes`, driven by `mesh.rs` through
`DecodeContext::begin_expand`.

Because alloc/work/depth are not yet charged at Rhino leaf sites, the peak-alloc
and work-charged columns below record what the fixtures *would* drive once the
count-framed loops adopt `exact_vec` — measured as reserved-collection bytes and
element reads on the existing fixtures — plus the one charge that is live today.

## Live charges (measured on in-tree fixtures)

| Dimension | Site | Fixture | Observation |
|---|---|---|---|
| `decompressed_bytes` | `mesh.rs` `begin_expand` | `mesh::tests::compressed_mesh` | inflated output charged incrementally per `ExpandWriter::write`; per-buffer and cumulative ceilings exercised by `cumulative_compressed_expansion_trips_the_platform_decompression_ceiling` |
| `retained_bytes` | `decode.rs` object retention | object-record fixtures | bounded by `RETAINED_RECORD_CAP` (16 MiB) and `RETAINED_DOCUMENT_CAP` (256 MiB); these codec-local caps are the defense-in-depth constants that survive the budget per doc section 4.4 |
| `alloc_bytes` + `work` | `cage.rs` knot/control loops | `cage::tests::decodes_rational_cage_order_knots_and_u_v_w_control_order` | 2x2x2 rational cage fixture: peak reserved = 3 knot vectors (2 f64 each) + 8 control tuples (`Vec<f64>` headers) + 8 stored tuples (4 f64) + 8 weights = ~0.6 KiB; `work` = Σ knot_count (6) + control_count*stored_dimension (32) = 38 units. Truncation exercised by `truncating_the_control_net_is_rejected_at_the_record_boundary` |
| `alloc_bytes` + `work` | `polyedge.rs` parameter/segment loops | `polyedge::tests::decodes_persistent_polyedge_segment_construction` | 1-segment / 2-parameter fixture: peak reserved = 2 f64 parameters (16 B) + 1 `Segment` (fixed struct) via `exact_vec`; `work` = parameter_count (2) + 13 fixed field reads per segment = 15 units. Truncation exercised by `truncating_the_segment_child_is_rejected_at_the_record_boundary` |
| `alloc_bytes` + `work` | `hatch.rs` header + boundary-loop table | `hatch::tests::decodes_version_two_loop_geometry_and_pattern_state` | 1-loop fixture (plane + one polyline loop): peak reserved = 1 `HatchLoop` via `exact_vec` plus any per-loop warnings via `grow_vec` (0 on the clean fixture); `work` = 19 fixed header field reads + loop_count (1) = 20 units. Truncation exercised by `truncating_the_loop_record_is_rejected_at_the_record_boundary` |

## Target charges (to be measured at graduation)

| Module | Peak reserved bytes (basis) | Work basis |
|---|---|---|
| `curves.rs` | Σ knot+CV counts × element size, per NURBS fixture | knot/CV reads + `ctx.descend` on embedded-curve nesting (`MAX_CURVE_DEPTH`) |
| `surfaces.rs` | Σ knot+CV-grid counts × element size | grid element reads |
| `mesh.rs` | vertex/face/channel counts × element size | channel element reads (decompression already charged) |
| `brep.rs` | Σ topology-table rows × element size | table rows + nodes+edges over the trim tree |
| `subd.rs` | control-net + fragment counts × element size | fragment element reads |
| `objects.rs`, `presentation.rs`, `dimensions.rs`, `instances.rs` | reserved attribute arrays | bytes examined per marker probe (not one unit per miss) |

## Method

Collect with the section 7 harness against the committed fuzz seeds and the
crate's golden fixtures, recording cumulative `alloc_bytes`/`work` and peak RSS
per graduated module set (doc section 10 Phase 2 performance gate). A
significant regression versus the pre-charge baseline requires explicit review
and must not be folded into a safety refactor. Populate the target table with
measured values as each module graduates; do not freeze alloc/work/depth for
Rhino until every high-risk count/recursion module reports real numbers here.
