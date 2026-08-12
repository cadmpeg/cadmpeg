# Phase 6 validate-extract compile spike

Measured at HEAD `5f9e72cf2` (BEFORE) and a same-tree stub trial (AFTER estimate).
Host: Darwin 24.6.0 arm64.
Validate module size: 16179 lines of 48338 in `cadmpeg-ir` (~33% of crate source).

## Designed public surface (required for extraction)

Extraction cannot keep `arena_registry!` as `pub(crate)` and expand it from another crate.
A stable boundary must cover the four current expand sites without duplicating arena membership:

1. **Census** — per-arena lengths (`validate.rs` registered census).
2. **Identity/order** — per-arena entity iteration with `EntitySchema::identity` (`identity_order.rs`).
3. **Referential integrity** — `(owner_identity, Reference)` walks (`referential_integrity.rs`).
   Existing `Model::visit_references` omits owner identity, so it is insufficient alone.
4. **Annotation projection** — serialize matching entities by id set (`annotations_native.rs`).

Eval stays in `cadmpeg-ir`. Extraction also requires promoting
`curve_parameter_near_point` (`eval.rs`, today `pub(crate)`) or an equivalent public
curve-parameter inverse with explicit tolerance. Other eval helpers validate uses are already `pub`.

That surface is not narrow: four distinct visitor contracts plus one new eval export.
Duplicating `arena_registry!` across a crate boundary is rejected.

Public eval API candidates (if extraction had proceeded):

```text
cadmpeg_ir::document::Model::arena_lens(&self) -> BTreeMap<&'static str, usize>
cadmpeg_ir::document::Model::visit_entities(&self, &mut dyn FnMut(&'static str, &str, &dyn EntitySchema))
cadmpeg_ir::document::Model::visit_owned_references(&self, &mut dyn FnMut(&str, Reference))
cadmpeg_ir::document::Model::entity_json_by_ids(&self, &HashSet<&str>) -> HashMap<String, Value>
cadmpeg_ir::eval::curve_parameter_near_point(...)  // promote pub(crate) -> pub
```

Plus relocating `CadIr::census` off `validate::entity_census` before the move (reverse edge).

## BEFORE (validate inside cadmpeg-ir)

| Graph | Clean (s) | Incremental (s) | Linked artifacts | Disk Δ (MB) |
|---|---:|---:|---:|---:|
| sat lib | 27.997 | 4.849 | 2 | 1045 |
| sat tests | 25.648 | 5.107 | 250 | 908 |
| CLI all-codecs | 49.602 | 21.939 | 2 | 2287 |
| test-fast workspace | 77.262 | 36.446 | 671 | 3719 |

`cadmpeg-ir` alone WITH validate: 28.958 s clean; `libcadmpeg_ir-*.rlib` 327.6 MB.

## AFTER estimate (validate stubbed out of cadmpeg-ir)

Trial: replace `validate.rs` + `validate/` with a documentation-only stub exporting the
same public symbols (empty admit sets, no-op reports). This upper-bounds the production
gain for a codec that does not call validation (sat). It is not a full `cadmpeg-validate`
crate trial; CLI / test-fast would still compile the moved 16k lines in a sibling crate.

| Graph | Clean (s) | Notes |
|---|---:|---|
| cadmpeg-ir alone | 24.616 | −4.3 s (−15%); rlib 291.2 MB (−36 MB) |
| sat lib | 25.231 | −2.8 s (−10% vs BEFORE 28.0 s) |

CLI, sldprt (writer gates), rhino/catia/iges (admit/reject), and `cargo test-fast` still
need the full validator. Their AFTER graph is `smaller ir` + `new validate crate`, so
wall time and linked work do not shrink materially and can grow at the crate boundary.

## Decision: do not create `cadmpeg-validate`

| Criterion | Result |
|---|---|
| Visitor/eval surface narrow? | No — four visitor contracts + eval promotion |
| Production rebuild win justifies surface? | No — ~10% on a non-validating codec lib; CLI/gated codecs immaterial |
| Test-graph win alone sufficient? | No (plan gate) |

`cadmpeg-validate` is not created. Validate remains in `cadmpeg-ir`.
Do not extract `eval` or `schema`. `roundtrip` stays in `cadmpeg-test-support`.
`diff` alone does not justify a crate.

sldprt would have kept a normal dependency if extracted (writer gates). Inventor stays out
of any validate packaging for symmetry reasons already satisfied by non-extraction.
