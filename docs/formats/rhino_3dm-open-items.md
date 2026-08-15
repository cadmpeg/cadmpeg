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
