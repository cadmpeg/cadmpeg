# Autodesk Fusion 360 `.f3d`: Open Items

This document records F3D semantics that the format specification does not yet define.

## Geometry carriers

- Law, taper, loft, skin, net, sweep, helix, vertex-blend, variable-blend, and compound-spline surface families lack defined carrier semantics. The primitive-reduction paths `plane/plane -> cylinder`, `plane/cylinder perpendicular -> torus`, and exact-circle-directrix cylinder also lack defined carrier semantics.
- The full closed-sphere and closed-torus seam conventions remain unspecified.
- The semantic role of the `POSITION` field after `cyl_spl_sur.extrusion_direction` is unresolved.
- The semantic role of the `ENUM_VALUE -1` field after the `rb_blend_spl_sur` radius pair is unresolved.

## Container, header, and design records

- The manifest relation that selects one asset folder when several asset folders are present is unresolved.
- The authoritative B-rep entry relation among multiple `.smb` or `.smbh` entries is unresolved. Filename extension, archive order, face count, and the relative size of the history partition do not define that relation.
- The relation between `.smb` and `.smbh` stream forms, including the presence of a history partition, is unresolved.
- The per-file-varying ASM header word at offset 24 has no assigned semantic meaning.
- The semantic meaning of `design_record_header_flag` is unspecified. Its relationship to UI visibility and explicit appearance assignment is unresolved.
- The semantic role of the second `0x01`-marker u32 in an ACT counter/registry record is unresolved.
- The terminating-group framing of multi-token `generic_tag_attrib_def` records is unresolved.

## Material assets

- `GenericSchema` InstanceProperties values form a schema-ordered vector. The serialization order does not follow raw XML declaration order, and the set of serialized fields is unspecified.
- The semantic identity of stored material presets, GUIDs, and protein phrases beyond their serialized values is unresolved.
- Precedence among face attributes, body attributes, design assignments, and `rh_material` records is unresolved.
