# Rhino 3DM Open Items

This document lists the parts of the Rhino 3DM format that we do not know. The specification `rhino_3dm.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Hostile sweep findings recorded on 2026-08-10

### TE-01. Object transfer on Rhino-authored files

**Question.** Why does an object class fail on a Rhino-authored file where the committed fixture for the same class passes?

**Known.** The external witness runs the codec and openNURBS over the example corpus and checks archive-level object totals and supported-count floors. It does not identify the byte-level difference for each class that remains undecoded.

**Note.** Reopened. The aggregate witness does not answer the item question. Treating corpus agreement with the current decoder as verification would be the consistency-as-verification failure this item was intended to prevent.

**Need.** We must find, for each affected class, which byte-level difference separates a Rhino-authored record from the fixture.

### TE-02. Witness strategy and the support claim

**Question.** Which files give an uncorrelated witness that the codec reads and writes 3DM?

**Known.** The branch adds an external openNURBS transfer test over the example corpus and pins aggregate floors by archive version. It does not add the requested synthesized second fixture tier or remeasure the full support claim with a per-version transfer requirement.

**Note.** Reopened. A test over the same corpus is useful regression evidence, but it does not supply the independent synthesized fixtures and support-boundary measurement required by this item.

**Need.** We need a second fixture tier that mirrors the example-file structure, plus a per-archive-version transfer measurement that defines the support claim.

### SW-03. Instance transform ownership inferred from topology

**Question.** Which decoded entities from one instance-definition member receive the instance transform?

**Known.** `crates/cadmpeg-codec-rhino/src/decode.rs:1940-1973` decodes one definition member and then calls `transform_new_entities`. At `decode.rs:1982-2107`, points referenced by new vertices, curves referenced by new edges, and surfaces referenced by new faces are classified as body-owned; other points, curves, and surfaces are transformed directly. Meshes and SubD entities are always transformed, and procedural curves and surfaces are omitted.

**Note.** The heuristic prevents double transformation for ordinary Brep topology, but no source field or independent witness establishes that topology attachment is the format's ownership rule for every member class.

**Need.** We need an instance fixture with mixed body and free member entities, plus an independent reader result or source membership rule that identifies which entities move and which stay local. If one member emits a body plus an auxiliary curve or surface whose source ownership is not represented by topology, the topology heuristic decides whether it moves. A shared or cache-like entity can therefore be transformed as free geometry, left in body-local coordinates, or omitted based on the emitted IR shape rather than the source member identity. No ownership ambiguity loss is emitted.

### SW-04. V1 vertex deduplication by first nearby point

**Question.** Does V1 topology identify shared vertices by source references or by geometric proximity?

**Known.** `crates/cadmpeg-codec-rhino/src/legacy.rs:578-612` builds IR vertices from endpoint coordinates and reuses the first existing vertex for which `same_point` succeeds. The comparison uses the maximum of the source tolerances and a fixed floor; it does not use a source vertex identifier.

**Note.** The code is an explicit first-match plausibility choice. The broad V1 grammar item LG-01 did not record this topology-selection rule.

**Need.** We need a source-backed V1 vertex identity rule and a fixture with distinct nearby vertices to test whether tolerance permits merging or only validates coordinates. If two distinct V1 vertex records lie within the selected tolerance but are topologically separate, the first endpoint inserted absorbs the second. Changing trim or face order changes the chosen IR vertex and can collapse a narrow edge or face.

### SW-05. V1 seam-group curve selection

**Question.** Which model-space curve owns a V1 seam or mate group when more than one trim stores one?

**Known.** `crates/cadmpeg-codec-rhino/src/legacy.rs:527-539` unions seam and shell mates, then stores only the first explicit curve for each union root with `or_insert_with`. It uses that curve's endpoints for the shared edge.

**Note.** Seam and mate records may be required to carry equivalent curves, but the current code does not verify that rule and the V1 ledger did not record the first-wins selection.

**Need.** We need the V1 seam/mate ownership rule and an independent pair of records with different curve copies to establish whether one is authoritative or disagreement is malformed. If two trims in one union group contain different model-space curve copies, source order selects the edge geometry and endpoints. A stale or transformed second copy is silently discarded, with no consistency check or loss.
