# Phase 1 PMI corpus differential

Measured at `e03375f0b` against Phase 0 HEAD `dc2ea5fee`.

## Scope

- 20 golden fixtures under `crates/cadmpeg-codec-sldprt/tests/golden/fixtures/`
- 1 CLI fixture `crates/cadmpeg/tests/fixtures/sldprt_triangle_body.sldprt`
- No additional `.sldprt` files under `corpus/` in this worktree
- Per-file decode timeout: 60s
- Comparison: `native.sldprt.pmi_dimensions` count plus projected fields
  `id,guid,cad_text,value,subtype,precision,display_text,basic,inspection,reference_only,value_offset,precision_offset` via `cadmpeg query`

## Result

| total | same | PMI content moved | failed |
| ----: | ---: | ----------------: | -----: |
|    21 |   21 |                 0 |      0 |

Only `pmi_semantic_dimension.sldprt` carries PMI dimensions in this set (count 1 before and after). The silent-loss population (array16+, reordered maps, key-like strings inside values) is not present in the checked-in fixtures; blast radius for release notes is therefore **zero documents changed** in available inputs, with the fix covered by new unit/integration fixtures and the `sldprt_pmi` fuzz target.

## Axes

- New `pmi.semantic-record-malformed` losses are additive under sidecar v1.
- `rmp = "=0.8.15"` is a pinned trust-boundary dependency used only for `Marker` / length reads.
