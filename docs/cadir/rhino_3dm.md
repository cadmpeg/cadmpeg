# Rhino 3DM: CADIR projection and writing

Layers, materials, appearance bindings, products, occurrences, external
references, views, annotations, text dots, arrows, embedded images, bitmaps,
document notes, revisions, and applications project to their neutral homes.
Layer facts belong to `presentation_layers`. The history fields `view_bytes`
and `view_sha256` describe source layout and are excluded from neutral feature
properties. Decoded features have `needs_recompute = false`.

The writer requests geometry and synthesizes a 3DM archive from the document.
Its export report states arena and native-record dispositions and names
omitted or reduced content.
