# IGES: CADIR projection and writing

Directory and Parameter Data records project to geometry, topology, product,
presentation, annotation, drawing, associativity, and property arenas. Levels
project to `presentation_layers`; property and attribute-table content projects
to `attributes`. Subfigures and networks project to product definitions and
occurrences. Primitive and procedural solids, Boolean trees, and selected
components use typed constructions; rectangular and circular arrays use typed
patterns. Decoded features have `needs_recompute = false`.

The writer requests geometry. It replays an eligible unedited source or
synthesizes from the document. Its report states arena and record dispositions.
A nonempty `unrepresented` selects synthesis.
