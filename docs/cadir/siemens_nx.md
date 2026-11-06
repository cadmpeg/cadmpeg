# Siemens NX: CADIR projection

The decoder projects supported geometry and topology, JT meshes to
`tessellations`, fast-load prototypes to `product_definitions`, and supported
feature constructions to their features. Parameter `unit`, `declaration`, and
`declaration_object_id` remain in typed native records; neutral maps omit these
copies and native record identifiers. Face colour definitions feed appearances.
Decoded features have `needs_recompute = false`. NX is read-only.
