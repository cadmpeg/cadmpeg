# IGES open items

This document lists the parts of the IGES format that we do not know, and the gaps in the evidence that the decoder rests on. The specification `iges.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

No unresolved byte meanings or structural rules are recorded.

## 1. Test evidence

### TE-01. Integration oracles over a fixture union

**Question.** Which arena must each integration fixture populate?

**Known.** `crates/cadmpeg-codec-iges/src/integration_tests.rs` holds seven integration tests. Each builds a list of fixtures, decodes them through `decode_matrix`, and then asserts a predicate with `results.iter().any(...)`. The assertion holds when one member of the list satisfies it, so the other members contribute nothing to that assertion. The `topology_pipeline` test, for example, decodes eight fixtures and asserts that some result has a non-empty `bodies` arena and that some result has a non-empty `pcurves` arena.

Two counterweights keep the severity at medium. `decode_matrix` decodes every fixture and calls `assert_valid` on each result, so each input passes decode and IR validation on its own. The unit suite in `src/tests.rs` carries per-entity assertions.

**Need.** We need a per-fixture expectation table that names the arena each input must populate. Without it, an entity family can stop decoding while every integration test stays green.
