# Autodesk Fusion 360 `.f3d`: Open Items

This document records F3D semantics that the format specification does not yet define.

## Geometry carriers

- The `law_spl_sur` family lacks complete carrier semantics. The primitive-reduction paths `plane/plane -> cylinder`, `plane/cylinder perpendicular -> torus`, and exact-circle-directrix cylinder also lack defined carrier semantics.
- The payload grammars for `crv_crv_v_bl_spl_sur`, `crv_srf_v_bl_spl_sur`, `sfcv_free_bl_spl_sur`, `VBL_OFFSURF` / `offsetvbsur`, `skin_spl_sur2`, and `sub_spl_sur` / `subsur` are undefined. These names cannot select the existing variable-blend, skin, or offset layouts without subtype-specific field boundaries.
- The basic surface record names `offset` and `sur-sur-int` are registered carrier names, but their record payloads and exact-geometry relations are undefined. They remain unknown surface carriers unless a spline subtype supplies a solved cache and construction graph.
- Variable-arity algebraic `readLaw` operators `MIN`, `MAX`, `SET`, `ROTATE`, and `STEP` have no defined serialized child-count or terminating delimiter. Their recursive boundaries cannot yet be decoded or written losslessly inside law, net, skin, and sweep payloads.
- The full closed-sphere and closed-torus seam conventions remain unspecified.
- The semantic role of the `POSITION` field after `cyl_spl_sur.extrusion_direction` is unresolved.
- The `subset_int_cur`, `comp_int_cur`, and labelled two-curve `offset_int_cur` field sequences with source curve blocks serialized flat before the cache lack real-stream confirmation. The observed `offset_int_cur` form opens with its cache and nests the progenitor curve in a trailing subtype scope, and its offset-law fields are unresolved.

## Container, header, and design records

- The top-level `Manifest.dat` field meanings and string records are defined, but the flag and padding bytes between `SimStructuralAttributes`, the asset-folder UUID, `FusionAssetName`, and `NA_EXPORT` have conflicting published offsets and no complete byte grammar. Canonical source-less manifest generation requires those bytes to be defined.
- The manifest relation that selects one asset folder when several asset folders are present is unresolved.
- The authoritative B-rep entry relation among multiple `.smb` or `.smbh` entries is unresolved. Filename extension, archive order, face count, and the relative size of the history partition do not define that relation.
- The relation between `.smb` and `.smbh` stream forms, including the presence of a history partition, is unresolved.
- History snapshot records reset their local record numbering after `End-of-ASM-History-Section`, while BulletinBoard `old` and `new` references use the construction-history revision namespace. The byte relation assigning each locally ordered snapshot record to its revision reference is unresolved; local ordinal order is not that relation. Historical model replay requires this assignment before changes can be reversed without dangling topology.

- The header flags word (both widths): bits above bit 0 have no assigned semantic meaning.
- The release word (both widths) encodes the ASM major release ×100 (`22700` on ASM 227.5, `23000` on ASM 230.5 streams); whether the minor release is ever encoded is unresolved.
- The entity-count word's counting rule (which records it counts) is unresolved in both widths.
- The semantic meaning of `design_record_header_flag` is unspecified. Its relationship to UI visibility and explicit appearance assignment is unresolved.
- The semantic role of the second `0x01`-marker u32 in an ACT counter/registry record is unresolved.
- The Design `MetaStream` Dimension object is a registry with no owned entity IDs. Paired-, counted-, and null-locus dimension frames resolve their sketch operands, but the remaining dimension companion variants and their locus arities are unresolved.
- The indexed parameter companion has a fixed prefix and an owner backlink. The semantic role of its opaque u64 and the payload grammar of non-locus companion variants are unresolved.
- Text-frame (`0x10000000000`) and text-path (`0x20000000000`) constraint bits exceed the settled u32 mask in the 101-byte sketch-relation record. The side-stream record carrying those 64-bit text-constraint masks is unresolved.
- The class-specific fields after the fixed `*_recipe_data` null sentinel and integer prologue are unresolved; their feature-operation, profile, extent, and dependency semantics are not assigned. Fillet and Chamfer edge operands resolve to ordered edge recipes, but the recipe fields assigning each operand to the active B-rep edge identity remain unresolved. Extrude scopes resolve their Sketch operand, distance/draft parameters, counted selection groups, and fixed-width selection members. The relation from each selection member's opaque u64 and two UUIDs to a sketch profile region or active B-rep identity, the Boolean operation, and non-distance termination operands remain unresolved. The semantic roles of the selection-group u32/f64 pair, variant byte, nested records, and the parameter-scope u32 immediately following its ordered reference table are also unresolved.
- The three Design body-bounding-box sextuples are value- and unit-defined, but the dynamic-class subrecord grammar that bounds each repetition and associates the repetition with its body is unresolved. Approximate offsets after an assignment container are not a structural decoder.

## Tolerant topology variants

- The `tedge` record inherits the base edge fields, but the byte position and unit semantics of its additional tolerance carrier are undefined.
- The `tcoedge` record carries `tStart` and `tEnd`, then version-selected boolean/reference fields and a variable tail containing integers or embedded curve records. The tail termination and embedded-record boundaries are undefined.

## Material assets

- `GenericSchema` InstanceProperties values form a schema-ordered vector. The serialization order does not follow raw XML declaration order, and the set of serialized fields is unspecified.
- The semantic identity of stored material presets, GUIDs, and protein phrases beyond their serialized values is unresolved.
- Precedence among face attributes, body attributes, design assignments, and `rh_material` records is unresolved.
