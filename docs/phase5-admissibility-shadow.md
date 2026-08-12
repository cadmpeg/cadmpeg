# Phase 5 admissibility shadow differential

Measured on `feat/refactor-convergence` after the admit predicate API landed and
before production gate swaps. Harness: `scripts/admissibility-shadow.py`.
Machine-readable rows: `docs/phase5-admissibility-shadow.json`.

## Scope

| route  |                                                      fixtures | predicate                                                        |
| ------ | ------------------------------------------------------------: | ---------------------------------------------------------------- |
| rhino  |  28 under `crates/cadmpeg-codec-rhino/tests/golden/fixtures/` | `RHINO_DRAFT_CHECKS` (core + Annotations; `ArenaOrder` excluded) |
| catia  |  19 under `crates/cadmpeg-codec-catia/tests/golden/fixtures/` | `CATIA_ADMISSION_CHECKS` (draft core)                            |
| iges   |   78 under `crates/cadmpeg-codec-iges/tests/golden/fixtures/` | full `validate_neutral` (and draft-core equivalence check)       |
| sldprt | 20 under `crates/cadmpeg-codec-sldprt/tests/golden/fixtures/` | `SLDPRT_EXPORT_PRECONDITION_CHECKS` (draft core)                 |

Per-file decode/validate timeout: 60s. Comparison: admit/reject from full
`cadmpeg validate` error/blocking findings versus the same findings filtered to
the route's `Check` set.

## Result

| total | agree_accept | agree_reject | diverge | decode_fail |
| ----: | -----------: | -----------: | ------: | ----------: |
|   145 |          143 |            0 |       0 |           2 |

Decode failures are intentional Rhino reject fixtures (`reject_v1.3dm`,
`reject_v2.3dm`) that do not produce IR. Zero admit/reject divergence on every
decoded fixture.

### IGES classification

Draft-core subset versus full `validate_neutral` on all 78 IGES golden fixtures:
**agree 78, diverge 0**. The rejection gate may keep full `validate_neutral`;
subset error outcomes are equivalent on the available corpus. Recorded as an
explicit keep-full decision, not an unexamined default.

## Gate switch criterion

Satisfied: zero admit/reject divergence on available golden corpora. Production
sites may switch onto the documented predicates.
