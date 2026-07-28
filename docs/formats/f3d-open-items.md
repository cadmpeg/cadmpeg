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

- Header flags bits 1 and above have no assigned semantic meaning.
- The semantic meaning of `design_record_header_flag` is unspecified. Its relationship to UI visibility and explicit appearance assignment is unresolved.
- The semantic role of the second `0x01`-marker u32 in an ACT counter/registry record is unresolved.
- Nonempty configuration-rule objects without paired string `when` and `activate` members have no defined activation grammar.
- The operation grammar relating the recipe records of a non-locus indexed-parameter-companion variant is unresolved.
- The semantics of sketch-relation member-role values are unresolved. Rectangular-pattern seed instances can contain both zero and nonzero roles, so role zero does not classify generated membership.
- The point-to-surface loci selected by sketch-relation member-role values `0` through `3` are unresolved. They do not select the four control-grid corners.
- The semantic meaning of the explicit per-member role integers within a `0x80000000` spline-group relation is unresolved.
- The value semantics of the sheet-metal `EdgeFlange` and `Hem` extent, height-datum, bend-position, direction, and hem-form discriminators are unresolved, so these features have no neutral operation grammar.
- `SpirePrimitive` section-placement values other than `4`, and the independent semantic name of its fixed u32 value `2` at primary-header offset 26, remain unresolved.
- In the `EntityGenesis`-form placement record class, the role of the f64-shaped field ending at primary-record offset 45 is unresolved.
- The construction-record join that determines the position of a reference-derived `WorkPoint` without an explicit class-282 coordinate is unresolved.
- The field semantics of the two patch-setting records at ordered reference positions two and three of the 354-byte `SurfacePatch` scope are unresolved.
- Recipe fields assigning operands with empty, disjoint, or multiply intersecting candidate sets to the active B-rep edge identity remain unresolved.
- When an Extrude face-recipe member has multiple active B-rep face candidates, the recipe field selecting one candidate remains unresolved.
- In Extrude selections, identities absent from history, the discriminator between multiple profile loops incident at the same ordered persistent Sketch points, the selector for one of several closed spatial-Sketch profiles, the context UUID's role, and the semantic role of the fixed member tail's optional slot remain unresolved.
- The construction-group scalar fields and variant byte, and the construction-group role field outside Extrude scopes, remain unresolved.
- The topology meaning of the root, prelude, and side-clause scalar fields shared by face-recipe nodes remains unresolved.
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

- The semantic roles of the u8 preceding a TextureURI path count and the u32 prelude before `texture_RealWorldOffsetX`, the complete Distance namespace, the unit-tag namespace beyond the inch and centimetre tags, and the application-defined meanings of texture map-channel integer values are unresolved.
- The Canvas fields carrying visibility, mirroring, and crop state are unresolved.
- The external material-library payloads keyed by preset phrases are unspecified; a preset phrase cannot be resolved to concrete appearance properties without the library.
- The precedence of `color-adesk-attrib`, `material-adesk-attrib`, and `rh_material` library records relative to direct colors and appearance assignments is unresolved.
