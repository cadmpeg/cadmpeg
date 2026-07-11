# Autodesk Fusion 360 `.f3d`: Open Items

This document records F3D semantics that the format specification does not yet define.

## Geometry carriers

- A rational `exp_par_cur` pcurve byte grammar is not defined. Explicit F3D pcurves use the non-rational `nubs` carrier; analytic-face coedges store a null pcurve reference.
- Law, taper, loft, skin, net, sweep, helix, vertex-blend, variable-blend, and compound-spline surface families lack defined carrier semantics. The primitive-reduction paths `plane/plane -> cylinder`, `plane/cylinder perpendicular -> torus`, and exact-circle-directrix cylinder also lack defined carrier semantics.
- The full closed-sphere and closed-torus seam conventions remain unspecified.
- The semantic role of the `POSITION` field after `cyl_spl_sur.extrusion_direction` is unresolved.
- The semantic role of the `ENUM_VALUE -1` field after the `rb_blend_spl_sur` radius pair is unresolved.

## Container, header, and design records

- The manifest relation that selects one asset folder when several asset folders are present is unresolved.
- The authoritative B-rep entry relation among multiple `.smb` or `.smbh` entries is unresolved. Filename extension, archive order, face count, and the relative size of the history partition do not define that relation.
- The relation between `.smb` and `.smbh` stream forms, including the presence of a history partition, is unresolved.
- The per-file-varying ASM header word at offset 24 has no assigned semantic meaning.
- The `BinaryFile4` header flags word: bits above bit 0 have no assigned semantic meaning (bit 2 is set on both observed stream forms).
- The `BinaryFile4` release word encodes the ASM major release ×100 (`22700` on ASM 227.5 streams); whether the minor release is ever encoded is unresolved.
- The `BinaryFile4` entity-count word's counting rule (which records it counts) is unresolved.
- The semantic meaning of `design_record_header_flag` is unspecified. Its relationship to UI visibility and explicit appearance assignment is unresolved.
- The semantic role of the second `0x01`-marker u32 in an ACT counter/registry record is unresolved.
- The terminating-group framing of multi-token `generic_tag_attrib_def` records is unresolved.
- The Design `MetaStream` Dimension object is a registry with no owned entity IDs. The location and byte grammar of concrete dimensional constraints and parameter expressions are unresolved.
- The class-specific fields after the fixed `*_recipe_data` null sentinel and integer prologue are unresolved; their feature-operation, profile, extent, and dependency semantics are not assigned.

## Material assets

- `GenericSchema` InstanceProperties values form a schema-ordered vector. The serialization order does not follow raw XML declaration order, and the set of serialized fields is unspecified.
- The semantic identity of stored material presets, GUIDs, and protein phrases beyond their serialized values is unresolved.
- Precedence among face attributes, body attributes, design assignments, and `rh_material` records is unresolved.
