# STEP Part 21: CADIR projection and writing

The decoder projects geometry, topology, products, tessellation, presentation,
and PMI. External references with a product or shape prototype project to
occurrences; other external references project to assets. Lengths use
millimetres. Decoded features have
`needs_recompute = false`.

The writer requests geometry and synthesizes the exchange from the document.
It reports arena and native-record dispositions, and names omitted or reduced
content. The selected application protocol determines the target vocabulary.

Equivalent appearance bindings for one body or face share a `STYLED_ITEM`.
Conflicting style projections are omitted with
`appearance.binding-target-conflict`, which names the target and bindings.
Visibility is written per binding; a target schema that cannot represent hidden
styles reports `presentation.hidden-appearance-visibility-unsupported`.
