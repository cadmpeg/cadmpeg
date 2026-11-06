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

The synthesized Global section declares millimetres, model scale 1, and 17
significant digits. Its minimum resolution uses the document's positive linear
tolerance and the geometry bound. The selected target determines the IGES
version field.

For IGES 4.0 and 5.0, a plane uses Type 108 Form 0 and a Directory
transformation. For IGES 5.1 through 5.3, it uses Type 190. Cylinders, cones,
spheres, and tori use Types 192, 194, 196, and 198 when they support B-rep
topology. The writer reports unsupported content as a reduction or omission.

For generated analytic frames, the writer admits a normalized axis dot-product
residual through `1e-6`, orthogonalizes the reference axis, and derives the
third axis. Its other geometric comparisons use `1e-10` for NURBS closedness
and same-value checks, `1e-8` for scale-relative endpoint admission, and
`1e-10` and `1e-12` for NURBS-plane construction checks.
