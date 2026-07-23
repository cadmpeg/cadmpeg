# Autodesk Fusion 360 `.f3d`: Open Items

This document records F3D semantics that the format specification does not yet define.

## Geometry carriers

- The payload grammars for `crv_crv_v_bl_spl_sur`, `crv_srf_v_bl_spl_sur`, `sfcv_free_bl_spl_sur`, `VBL_OFFSURF` / `offsetvbsur`, and `skin_spl_sur2` are undefined. A valid final solved cache supplies the exact face shape while the complete construction record remains opaque. Cacheless occurrences cannot select the existing variable-blend, skin, or offset layouts without subtype-specific field boundaries.
- The basic surface record names `offset` and `sur-sur-int` are registered carrier names, but their record payloads and exact-geometry relations are undefined. They remain unknown surface carriers unless a spline subtype supplies a solved cache and construction graph.
- The roles of the two boolean flags terminating a cache-first `par_int_cur` are unresolved. The second flag is false in every observed instance; the first varies.
- The roles of the four booleans between the offset distance and the enum in a revision-gated `off_spl_sur` are unresolved, including which of them carry the U/V senses and which belong to the ASM extension tail. One observed instance with true third and fourth flags carries an additional run before the shared tail — a boolean, six integers, a boolean, an embedded cache-first intcurve with optional endpoints, further booleans, a small tolerance scalar, and four `-1` integers; which flag gates the run and the field roles are unresolved, and such records are retained verbatim.
- The role of the boolean following the shared revision-gated surface tail in a revision-gated `ortho_spl_sur` is unresolved, as is which of the two trailing booleans is the orthogonal sense. Both trailing booleans are false in every observed instance.
- The role of the enum opening the shared revision-gated surface tail is unresolved.
- Variable-arity algebraic `readLaw` operators `MIN`, `MAX`, and `STEP` have no defined serialized child-count or terminating delimiter. Their recursive boundaries cannot yet be decoded or written losslessly inside law, net, skin, and sweep payloads.
- The semantic role of the integer between the secondary and tertiary pcurves in a variable-blend support side is unresolved; it is zero in every observed side.
- The four optional parameter values between the shared revision-gated surface tail and the trailing enum of revision-gated `exact_spl_sur` and `t_spl_sur` are `(1, 0, 1, 0)` in every observed instance regardless of the cache knot domains; their semantics are unresolved.
- The semantic roles and coordinate ordering of the four optional scalar fields between the sections and flags of revision-gated `loft_spl_sur` are unresolved.
- Revision-gated `cl_loft_spl_sur` tail kinds other than zero are unobserved and undefined. The condition selecting the optional trailing values and BS3 curve of the kind-zero payload beyond their structural presence is unresolved.
- Whether a pre-revision `var_blend_spl_sur` / `srf_srf_v_bl_spl_sur` layout exists in which the leading integer is a subtype definition-table index rather than the serializer revision is unresolved.
- Which of the three `fixed_width` blend-value scalars are the endpoint parameters and which is the width is unresolved.
- The token tags of a revision-gated `VBL_SURF` `deg` boundary are unobserved.
- The semantic roles of the variable-blend tail Boolean and of the three integers that follow it are unresolved.

## Container, header, and design records

- The relation between `.smb` and `.smbh` stream forms, including the presence of a history partition, is unresolved.
- The header flags word (both widths): bit 1 (always set) and bits 2 and above (always zero) have no assigned semantic meaning.
- The release word (both widths) encodes the ASM major release ×100 (`22700` on ASM 227.5, `23000` on ASM 230.5 streams); whether the minor release is ever encoded is unresolved.
- The semantic meaning of `design_record_header_flag` is unspecified. Its relationship to UI visibility and explicit appearance assignment is unresolved.
- The semantic role of the second `0x01`-marker u32 in an ACT counter/registry record is unresolved.
- Nonempty configuration-rule objects without paired string `when` and `activate` members have no defined activation grammar.
- The Design `MetaStream` Dimension object is a registry with no owned entity IDs. Paired-, repeated counted-, null-locus, annotated `EntityGenesis`, and recipe-backed dimension frames retain their operands.
- The indexed parameter companion has a fixed prefix, an owner backlink, a Unix-epoch microsecond timestamp, an exact owned interval, and an ordered set of contained construction recipes. Dimension-owned recipes resolve to their immediate indexed-record containers. Whether the timestamp denotes parameter creation or last modification is unresolved, and the operation grammar relating recipe records in non-locus companion variants is unresolved.
- The semantics of sketch-relation member-role values are unresolved. Rectangular-pattern seed instances can contain both zero and nonzero roles, so role zero does not classify generated membership.
- The point-to-surface loci selected by sketch-relation member-role values `0` through `3` are unresolved. They do not select the four control-grid corners.
- The semantic meaning of the explicit per-member role integers within a `0x80000000` spline-group relation is unresolved.
- Whether `EntityGenesis`-form sketch coordinate values follow the document display unit or are fixed at ten times the centimetre value is unresolved.
- In a sketch member-run head record, the roles of the eleven zero bytes between the record index and the placement matrix and of the marked tail after the matrix are unresolved.
- Sheet-metal `EdgeFlange` and `Hem` have exact edge, parameter-owner, settings, and bend-radius frames, but the extent, height-datum, bend-position, direction, and hem-form discriminator meanings are unresolved, so they have no neutral operation grammar.
- `SpirePrimitive` section-placement values other than `4`, and the independent semantic name of its fixed u32 value `2` at primary-header offset 26, remain unresolved.
- In the `EntityGenesis`-form placement record class, the role of the f64-shaped field ending at primary-record offset 45, the record referenced at offset 57 of the 362-byte WorkPlane variant, and the shared tail fields of the 213- and 341-byte sketch placement forms are unresolved.
- The construction-record join that determines the position of a reference-derived `WorkPoint` without an explicit class-282 coordinate is unresolved.
- The field semantics of the two patch-setting records at ordered reference positions two and three of the 354-byte `SurfacePatch` scope are unresolved.
- The class-specific fields after the fixed `*_recipe_data` null sentinel and integer prologue are unresolved; their feature-operation, profile, extent, and dependency semantics are not assigned. Fillet and Chamfer edge operands resolve to counted groups of ordered edge recipes; Fillet groups resolve their radius and tangency-weight inputs, and Chamfer groups resolve their independent dimensional specifications. Equal edge-recipe entry selectors group topology-context entries across the two clauses. The assignment semantics when the two clauses carry different selectors remain unresolved; the clauses cannot be treated as independent selected-edge identities because their unique incidence candidates can name different edges. Each topology triplet names one loop vertex and its preceding or following incident edge. A unique intersection across every selector incidence set and every available persistent-reference face-adjacency set resolves the operand's exact historical edge. A unique intersection of all triplet-named historical edges with the preceding boundary edges deleted by the feature transition also resolves the operand. Recipe fields assigning operands with empty, disjoint, or multiply intersecting candidate sets to the active B-rep edge identity remain unresolved. Extrude face recipes join their persistent Design reference to a deterministic set of active B-rep face candidates; when a member has multiple candidates, the recipe field selecting one candidate remains unresolved. Extrude scopes resolve their result operation, direction reversal, profile-plane, offset-profile-plane, and selected-face starts, one-sided distance, one-sided to-face, and two-sided distance forms, Sketch operand, distance/draft parameters, body/profile/face construction-operand roles, ordered start and termination face groups, counted construction-operand and selection groups, nested operand-identity chains, fixed persistent identities, fixed-width selection members, the exact identity chains terminating at each selection member, invariant stable ASM history families, member identities that name persistent geometry in the selected Sketch, and historical loop, coedge, edge, vertex, point, curve, and pcurve identities whose projected vertex positions uniquely select a line, circle, bounded arc, bounded ellipse, or nonperiodic NURBS profile. Selection identities absent from history, the discriminator between multiple profile loops incident at the same ordered persistent Sketch points, the selector for one of several closed spatial-Sketch profiles, the context UUID's role, and the semantic role of the fixed member tail's optional slot remain unresolved. The construction-group scalar fields and variant byte, and the role field outside Extrude scopes, remain unresolved.
- The topology meaning of the root, prelude, side-clause scalar, and entry fields shared by face-recipe nodes remains unresolved.
- Outside a role-`0x0000000500000000` spatial-Sketch Loft section or guide, the individual entity roles of the two u64 values in a nested Loft entity-selection member's `N+3` record remain unresolved.
- The semantic roles of the class-365 whole-body operand fields after its asset and context UUIDs, excluding the bounded nested-record and body-recipe joins, remain unresolved.
- The semantic roles of the six-byte fields following Base Feature body suffixes and record references are unresolved.
- The semantic roles of the f64 and two f32 fields between a sketch-text record's nominal height and font family, its two internal record references, and its class-specific tail fields are unresolved.

- The individual scalar and index roles within `0m cg` derived-grip records, and the direct cage-object identity join needed to partition active TSM entries between multiple Form scopes, are unresolved.

## External references

- The semantics of `neutronData` when its GUID differs from `neutronRole` are unresolved.
- The grammar of a non-empty `ComponentReferenceData.json` is unresolved.
- The role of the `0x01`-tagged eight-byte value preceding the owning-design GUID in a `DcXRefPCIFeature` record is unresolved.
- The semantic roles of the u32 fields in the role-adjacent occurrence-placement tail are unresolved.

## Material assets

- The semantic roles of the u8 preceding a TextureURI path count and the u32 prelude before `texture_RealWorldOffsetX`, the complete Distance and unit-tag namespaces, and the application-defined meanings of texture map-channel integer values are unresolved.
- Canvas records resolve their image asset, supporting construction-plane entity, owning component, and plane-local rectangular bounds. The meanings of the 77-byte geometry payload and the differing scope and geometry prologue markers, including opacity, visibility, mirroring, and crop state, are unresolved. The construction-plane entity's exact model-space frame join and the Design-record grammar for decal objects remain unresolved.
- The resolution of preset phrases against the external material library is unresolved.
- The precedence of `color-adesk-attrib`, `material-adesk-attrib`, and `rh_material` library records relative to direct colors and appearance assignments is unresolved.
