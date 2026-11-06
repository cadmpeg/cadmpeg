# Siemens NX: CADIR projection

The decoder projects supported geometry and topology, JT meshes to
`tessellations`, fast-load prototypes to `product_definitions`, and supported
feature constructions to their features. Fast-load occurrences and external
references project to occurrences. JT material records project to appearances;
JT string-property atoms and non-topological Parasolid attributes project to
attributes. Assembly placement records stay native until their occurrence binding
and translation unit are established. Parameter `unit`, `declaration`, and
`declaration_object_id` remain in typed native records; neutral maps omit these
copies and native record identifiers. Face colour definitions feed appearances.
Decoded features have `needs_recompute = false`. NX is read-only.
