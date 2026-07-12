# Autodesk Fusion 360 `.f3d`: Open Items

This document records F3D semantics that the format specification does not yet define.

## Geometry carriers

- A rational `exp_par_cur` pcurve byte grammar is not defined. Explicit F3D pcurves use the non-rational `nubs` carrier; analytic-face coedges store a null pcurve reference.
- Law, taper, loft, skin, net, sweep, helix, vertex-blend, variable-blend, and compound-spline surface families lack defined carrier semantics. The primitive-reduction paths `plane/plane -> cylinder`, `plane/cylinder perpendicular -> torus`, and exact-circle-directrix cylinder also lack defined carrier semantics.
- The full closed-sphere and closed-torus seam conventions remain unspecified.
- The semantic role of the `POSITION` field after `cyl_spl_sur.extrusion_direction` is unresolved.
- The semantic role of the `ENUM_VALUE -1` field after the `rb_blend_spl_sur` radius pair is unresolved.
- The `subset_int_cur`, `comp_int_cur`, and labelled two-curve `offset_int_cur` field sequences with source curve blocks serialized flat before the cache lack real-stream confirmation. The observed `offset_int_cur` form opens with its cache and nests the progenitor curve in a trailing subtype scope, and its offset-law fields are unresolved.
- Whether a pcurve's UV coordinates live on the owning surface's exact procedural parameterization or on its B-spline cache parameterization is unresolved. On `rot_spl_sur` faces the stored UV values follow the exact (angle × profile-parameter) space, which drifts from the rational cache parameterization between knots.

## Container, header, and design records

- The manifest relation that selects one asset folder when several asset folders are present is unresolved.
- The authoritative B-rep entry relation among multiple `.smb` or `.smbh` entries is unresolved. Filename extension, archive order, face count, and the relative size of the history partition do not define that relation.
- The relation between `.smb` and `.smbh` stream forms, including the presence of a history partition, is unresolved.

- The header flags word (both widths): bits above bit 0 have no assigned semantic meaning.
- The release word (both widths) encodes the ASM major release ×100 (`22700` on ASM 227.5, `23000` on ASM 230.5 streams); whether the minor release is ever encoded is unresolved.
- The entity-count word's counting rule (which records it counts) is unresolved in both widths.
- The semantic meaning of `design_record_header_flag` is unspecified. Its relationship to UI visibility and explicit appearance assignment is unresolved.
- The storage location of per-body UI visibility is unresolved. The `BodiesRoot` member `u16` flag word is zero for a shown body and for a hidden body in the same stream, so it does not carry visibility. A B-rep stream can contain solid bodies that Fusion does not display; without the visibility relation every body in the active stream decodes as model geometry.
- The semantic role of the second `0x01`-marker u32 in an ACT counter/registry record is unresolved.
- The terminating-group framing of multi-token `generic_tag_attrib_def` records is unresolved.
- The Design `MetaStream` Dimension object is a registry with no owned entity IDs. The location and byte grammar of concrete dimensional constraints and parameter expressions are unresolved.
- Text-frame (`0x10000000000`) and text-path (`0x20000000000`) constraint bits exceed the settled u32 mask in the 101-byte sketch-relation record. The side-stream record carrying those 64-bit text-constraint masks is unresolved.
- The class-specific fields after the fixed `*_recipe_data` null sentinel and integer prologue are unresolved; their feature-operation, profile, extent, and dependency semantics are not assigned.

## Material assets

- `GenericSchema` InstanceProperties values form a schema-ordered vector. The serialization order does not follow raw XML declaration order, and the set of serialized fields is unspecified.
- The semantic identity of stored material presets, GUIDs, and protein phrases beyond their serialized values is unresolved.
- Precedence among face attributes, body attributes, design assignments, and `rh_material` records is unresolved.
