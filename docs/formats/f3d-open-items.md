# Autodesk Fusion 360 `.f3d`: Open Items

This document records F3D semantics that the format specification does not yet define.

## Geometry carriers

- The `summary`, `none`, `historical`, and `optimal` standard-tail modes of `law_spl_sur` are undefined. The `full` tail is defined and carries a solved NURBS surface.
- The payload grammars for `crv_crv_v_bl_spl_sur`, `crv_srf_v_bl_spl_sur`, `sfcv_free_bl_spl_sur`, `VBL_OFFSURF` / `offsetvbsur`, `skin_spl_sur2`, and `sub_spl_sur` / `subsur` are undefined. These names cannot select the existing variable-blend, skin, or offset layouts without subtype-specific field boundaries.
- The basic surface record names `offset` and `sur-sur-int` are registered carrier names, but their record payloads and exact-geometry relations are undefined. They remain unknown surface carriers unless a spline subtype supplies a solved cache and construction graph.
- Variable-arity algebraic `readLaw` operators `MIN`, `MAX`, and `STEP` have no defined serialized child-count or terminating delimiter. Their recursive boundaries cannot yet be decoded or written losslessly inside law, net, skin, and sweep payloads.
- The native loop and degenerate-edge layouts for untrimmed closed spheres and tori are unspecified.
- The semantic role of the `POSITION` field after `cyl_spl_sur.extrusion_direction` is unresolved.

## Container, header, and design records

- The top-level `Manifest.dat` field meanings and string records are defined, but the flag and padding bytes between `SimStructuralAttributes`, the asset-folder UUID, `FusionAssetName`, and `NA_EXPORT` have conflicting published offsets and no complete byte grammar. Canonical source-less manifest generation requires those bytes to be defined.
- The manifest relation that selects one asset folder when several asset folders are present is unresolved.
- The authoritative B-rep entry relation among multiple `.smb` or `.smbh` entries is unresolved. Filename extension, archive order, face count, and the relative size of the history partition do not define that relation.
- The relation between `.smb` and `.smbh` stream forms, including the presence of a history partition, is unresolved.
- The header flags word (both widths): bits above bit 0 have no assigned semantic meaning.
- The release word (both widths) encodes the ASM major release ×100 (`22700` on ASM 227.5, `23000` on ASM 230.5 streams); whether the minor release is ever encoded is unresolved.
- The entity-count word's counting rule (which records it counts) is unresolved in both widths.
- The semantic meaning of `design_record_header_flag` is unspecified. Its relationship to UI visibility and explicit appearance assignment is unresolved.
- The semantic role of the second `0x01`-marker u32 in an ACT counter/registry record is unresolved.
- Nonempty configuration-rule objects without paired string `when` and `activate` members have no defined activation grammar.
- The Design `MetaStream` Dimension object is a registry with no owned entity IDs. Paired-, repeated counted-, null-locus, and recipe-backed dimension frames retain their operands, but payload-bearing dimension companion variants containing neither locus nor recipe frames are unresolved.
- The indexed parameter companion has a fixed prefix, an owner backlink, a Unix-epoch microsecond timestamp, an exact owned interval, and an ordered set of contained construction recipes. Dimension-owned recipes resolve to their immediate indexed-record containers. The application event denoted by the timestamp and the operation grammar relating recipe records in non-locus companion variants are unresolved.
- Text-frame (`0x10000000000`) and text-path (`0x20000000000`) constraint bits exceed the settled u32 mask in the 101-byte sketch-relation record. The side-stream record carrying those 64-bit text-constraint masks is unresolved.
- The member, auxiliary, and return-reference roles within circular-pattern and rectangular-pattern sketch relations are unresolved.
- The class-specific fields after the fixed `*_recipe_data` null sentinel and integer prologue are unresolved; their feature-operation, profile, extent, and dependency semantics are not assigned. Fillet and Chamfer edge operands resolve to counted groups of ordered edge recipes; Fillet groups resolve their radius and tangency-weight inputs, and Chamfer groups resolve their independent dimensional specifications. Equal edge-recipe entry selectors group topology-context entries across the two clauses. Each topology triplet names one loop vertex and its preceding or following incident edge. A unique intersection across every selector incidence set and every available persistent-reference face-adjacency set resolves the operand's exact historical edge. The role of an ordered triplet pair naming two different incident-edge ordinals remains unresolved. Recipe fields assigning operands with empty, disjoint, or multiply intersecting candidate sets to the active B-rep edge identity remain unresolved. Extrude face recipes join their persistent Design reference to a deterministic set of active B-rep face candidates; when a member has multiple candidates, the recipe field selecting one candidate remains unresolved. Extrude scopes resolve their result operation, direction reversal, profile-plane, offset-profile-plane, and selected-face starts, one-sided distance, one-sided to-face, and two-sided distance forms, Sketch operand, distance/draft parameters, body/profile/face construction-operand roles, ordered start and termination face groups, counted construction-operand and selection groups, nested operand-identity chains, fixed persistent identities, fixed-width selection members, the exact identity chains terminating at each selection member, invariant stable ASM history families, member identities that name persistent geometry in the selected Sketch, and historical loop, coedge, edge, vertex, point, curve, and pcurve identities whose projected vertex positions uniquely select a line, circle, bounded arc, bounded ellipse, or clamped NURBS profile. Selection identities absent from history and the context UUID's role remain unresolved. The construction-group scalar fields and variant byte, and the role field outside Extrude scopes, remain unresolved.
- The topology meaning of the root, prelude, side-clause scalar, and entry fields shared by face-recipe nodes remains unresolved.

## Tolerant topology variants

- The semantic role of the boolean before a modern `tcoedge` selector-one curve is unresolved.

## Material assets

- `GenericSchema` InstanceProperties values form a schema-ordered vector. The serialization order does not follow raw XML declaration order, and the set of serialized fields is unspecified.
- The semantic identity of stored material presets, GUIDs, and protein phrases beyond their serialized values is unresolved.
- The precedence of `color-adesk-attrib`, `material-adesk-attrib`, and `rh_material` library records relative to direct colors and appearance assignments is unresolved.
