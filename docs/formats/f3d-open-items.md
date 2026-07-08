# Autodesk Fusion 360 `.f3d`: Open Items

This document records F3D semantics that the format specification does not yet define.

## Geometry carriers

- Law, taper, loft, skin, net, sweep, helix, vertex-blend, variable-blend, and compound-spline surface families lack defined carrier semantics. The primitive-reduction paths `plane/plane -> cylinder`, `plane/cylinder perpendicular -> torus`, and exact-circle-directrix cylinder also lack defined carrier semantics.
- The full closed-sphere and closed-torus seam conventions remain unspecified.

## Header and design records

- The per-file-varying ASM header word at offset 24 has no assigned semantic meaning.
- The semantic meaning of `design_record_header_flag` is unspecified. Its relationship to UI visibility and explicit appearance assignment is unresolved.
- The semantic role of the second `0x01`-marker u32 in an ACT counter/registry record is unresolved.
- The terminating-group framing of multi-token `generic_tag_attrib_def` records is unresolved.

## Material assets

- `GenericSchema` InstanceProperties values form a schema-ordered vector. The serialization order does not follow raw XML declaration order, and the set of serialized fields is unspecified.
- The `.f3d` bytes do not contain the mapping from material preset or GUID to Autodesk material-library display name. Resolving display names requires an external material-library catalog.
