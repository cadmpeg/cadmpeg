# Autodesk Fusion 360 `.f3d`: Open Items

This document records F3D semantics that the format specification does not yet define.

## Geometry carriers

- The payload grammars for `VBL_OFFSURF` / `offsetvbsur` and `skin_spl_sur2` are undefined. A valid final solved cache supplies the exact face shape while the complete construction record remains opaque. Cacheless occurrences cannot select the existing vertex-blend offset or skin layouts without subtype-specific field boundaries.
- The basic surface record names `offset` and `sur-sur-int` are registered carrier names, but their record payloads and exact-geometry relations are undefined. They remain unknown surface carriers unless a spline subtype supplies a solved cache and construction graph.
- The role of the second boolean flag terminating a cache-first `par_int_cur` is unresolved. It takes both values, varies within a single stream, and has no identified correlate.
- Which of the leading two-boolean sense pair in a revision-gated `off_spl_sur` carries the U sense and which the V sense, and the meaning of the following two-boolean ASM extension prefix, are unresolved. An instance whose extension-prefix flags are both true carries an additional run before the shared tail — a boolean, six integers, a boolean, an embedded cache-first intcurve with optional endpoints, further booleans, a small tolerance scalar, and four `-1` integers; which flag gates the run and the field roles are unresolved, and such records are retained verbatim.
- The role of the boolean following the shared revision-gated surface tail in a revision-gated `ortho_spl_sur` is unresolved. The shared-tail boolean is false in every observed instance; the final boolean varies and positionally corresponds to the single boolean of the text form, but which boolean is the orthogonal sense remains unresolved.
- The role of the enum opening the shared revision-gated surface tail is unresolved. Only its zero value has a defined tail grammar; a non-zero value selects a native branch in which the containing record is retained verbatim.
- Variable-arity algebraic `readLaw` operators `MIN`, `MAX`, and `STEP` have no defined serialized child-count or terminating delimiter. Their recursive boundaries cannot yet be decoded or written losslessly inside law, net, skin, and sweep payloads.
- The semantic role of the integer between the secondary and tertiary pcurves in a variable-blend support side is unresolved; it is zero whether the secondary pcurve is null or present, and the tertiary pcurve slot is null in every observed side.
- The semantic roles of the four optional parameter values between the shared revision-gated surface tail and the trailing enum of revision-gated `exact_spl_sur` and `t_spl_sur` are unresolved.
- The semantic roles within each of the two scalar pairs between the sections and flags of revision-gated `loft_spl_sur` are unresolved.
- Revision-gated `cl_loft_spl_sur` tail kinds other than zero are unobserved and undefined. The condition selecting the optional trailing values and BS3 curve of the kind-zero payload beyond their structural presence is unresolved.
- Whether a pre-revision `var_blend_spl_sur` / `srf_srf_v_bl_spl_sur` layout exists in which the leading integer is a subtype definition-table index rather than the serializer revision is unresolved.
- The token tags of a revision-gated `VBL_SURF` `deg` boundary are unobserved.
- The semantic roles of the variable-blend tail Boolean and of the three integers that follow it are unresolved.
- Blend-value payloads have incomplete selector namespaces: two-radii chamfer-selector values other than `0` and `3`, single-radius selector values other than `0`, `1`, and `7`, what distinguishes single-radius selector `1` from `7`, and the semantics of the optional `interp` scalar-pair tail are unresolved.
- The role of the second of the two leading `tvertex` tolerance slots is unresolved. The first slot is the unevaluated sentinel `-1` in every observed instance; the second is an earlier tolerance evaluation satisfying second ≤ third ≤ second + 1e-6, where the third slot is the vertex tolerance.
- The `tedge` trailing LONG (`chunk[13]`) is version-gated, absent in older streams and taking values `0` and `1` when present. It is retained verbatim; its role is unresolved.

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
- `SpirePrimitive` section-placement values other than `4`, `CoilPrimitive` operation values other than `1`, extent values other than `1`, section values other than `1`, section-placement values other than `3`, and the independent semantic names of their fixed u32 values at primary-header offset 26 remain unresolved.
- The semantic role of the eighth ordered `CoilPrimitive` scope reference is unresolved.
- In the `EntityGenesis`-form placement record class, the role of the f64-shaped field ending at primary-record offset 45 is unresolved.
- The construction-record join that determines the position of a reference-derived `WorkPoint` without an explicit class-282 coordinate is unresolved.
- The field semantics of the two patch-setting records at ordered reference positions two and three of the 354-byte `SurfacePatch` scope are unresolved.
- Recipe fields assigning operands with empty, disjoint, or multiply intersecting candidate sets to the active B-rep edge identity remain unresolved.
- When an Extrude face-recipe member has multiple active B-rep face candidates, the recipe field selecting one candidate remains unresolved.
- In Extrude selections, identities absent from history, the discriminator between multiple profile loops incident at the same ordered persistent Sketch points, the selector for one of several closed spatial-Sketch profiles, the context UUID's role, and the semantic role of the fixed member tail's optional slot remain unresolved.
- Shifted Extrude extent-discriminator pairs other than `(1, 1)`, `(1, 2)`, `(2, 0)`, and `(3, 2)` remain unresolved.
- The semantic roles of additional Extrude construction groups with role `0x0000000500000000` remain unresolved.
- The construction-group scalar fields and variant byte remain unresolved. Group-role fields outside the defined feature-specific namespaces remain unresolved.
- The topology meaning of the root, prelude, and side-clause scalar fields shared by face-recipe nodes remains unresolved.
- The join from a `Move` or `RemoveBody` role-`0x0000000400000000` construction-group identity to neutral body identities is unresolved; the group identity is retained as the native body selection.
- The join from each `Combine` body-selection record's GUID pair and remaining native fields to a neutral body identity is unresolved; the ordered record identities are retained as the target and tool selections.
- The relationship between a `Draft` scope's signed angle, neutral-plane orientation, explicit pull direction, and outward-material convention is unresolved. The signed angle and both face selections are projected without inventing the redundant direction fields.
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
