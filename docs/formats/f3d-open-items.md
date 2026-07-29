# Autodesk Fusion 360 `.f3d`: Open Items

This document records F3D semantics that the format specification does not yet define.

## Geometry carriers

- The payload grammars for `VBL_OFFSURF` / `offsetvbsur` and `skin_spl_sur2` are undefined. A valid final solved cache supplies the exact face shape while the complete construction record remains opaque. Cacheless occurrences cannot select the existing vertex-blend offset or skin layouts without subtype-specific field boundaries.
- The basic surface record names `offset` and `sur-sur-int` correspond to the `off_spl_sur` construction and to the surface–surface-intersection construction graph carried by other records; their standalone payload grammars remain undefined.
- The role of the second boolean flag terminating a cache-first `par_int_cur` is unresolved. It takes both values, varies within a single stream, and has no identified correlate.
- The condition under which a cache-first `par_int_cur` elides its second pcurve slot — serializing a single null pcurve where the layout otherwise carries two ordered pcurve slots — is unresolved.
- The leading two-boolean pair of a revision-gated `off_spl_sur` carries record-level progenitor sense state rather than a per-axis U/V decomposition; no instance requires a per-axis reading, and the flags do not perturb the solved cache. Whether the first flag is the reversal state and the second the reflection state, or the converse, is unresolved, and the meaning of the following two-boolean ASM extension prefix is unresolved. An instance whose extension-prefix flags are both true carries an additional run before the shared tail — a boolean, six integers, a boolean, an embedded cache-first intcurve with optional endpoints, further booleans, a small tolerance scalar, and four `-1` integers; which flag gates the run and the field roles are unresolved, and such records are retained verbatim. The run's leading fields correspond structurally to a surface–surface-intersection payload prefix — presence logical, six-integer header, intersection curve, two pcurve logicals, and tolerance — but the trailing four `-1` integers exceed the three endpoint-term slots of that grammar, so the run grammar remains unresolved.
- The role of the enum opening the shared revision-gated surface tail is unresolved. Its zero value selects the full solved-cache tail; nonzero branch grammars remain undefined, and a nonzero value selects a native branch in which the containing record is retained verbatim.
- Variable-arity algebraic `readLaw` operators `MIN`, `MAX`, and `STEP` have no observed framed serialization. `STEP` occurs only inside stored law formula strings, which are retained as text with no framed node, child count, or delimiter; `MIN` and `MAX` occur in no observed law payload in any form. Whether the framed law-expression grammar of net, skin, and sweep payloads admits these operators, and with what recursive boundary, is unresolved; native generation of framed expressions using them remains rejected.
- The semantic role of the integer between the secondary and tertiary pcurves in a variable-blend support side is unresolved; it is zero whether the secondary pcurve is null or present, and the tertiary pcurve slot is null in every observed side.
- Which of the two unextended parameter intervals of revision-gated `exact_spl_sur` and `t_spl_sur` corresponds to the U direction and which to the V direction is unresolved.
- Which of the two revision-gated `loft_spl_sur` wrap-range intervals corresponds to which closure direction of the solved surface is unresolved; the interval order does not align with a first-interval-is-U reading of the cache closure enums.
- The revision-gated `cl_loft_spl_sur` tail kind is a tail-kind integer of which only the zero value has a defined grammar; the nonzero vocabulary and the condition selecting the optional trailing values and BS3 curve of the kind-zero payload remain unresolved.
- Whether a pre-revision `var_blend_spl_sur` / `srf_srf_v_bl_spl_sur` layout exists in which the leading integer is a subtype definition-table index rather than the serializer revision is unresolved.
- The token tags of a revision-gated `VBL_SURF` `deg` boundary are unobserved.
- The value namespaces of the variable-blend tail integers beyond their settled roles are unresolved: approximation-current values other than `1`, count values other than `1`, and the field content selected by a nonzero count are undefined.
- Blend-value payloads have incomplete selector namespaces: two-radii chamfer-selector values other than `0` and `3`, single-radius selector values other than `0`, `1`, and `7`, what distinguishes single-radius selector `1` from `7`, and the semantics of the optional two-scalar `interp` tail selected by its trailing flag are unresolved.
- The selection rule among the three independent `tvertex` tolerance evaluations is unresolved. The first slot is the unset sentinel `-1` in every observed instance; the second is unset or an earlier evaluation satisfying second ≤ third ≤ second + 1e-6, where the third slot is the vertex tolerance.
- The `tedge` trailing LONG (`chunk[13]`), following the per-entity serializer revision stamp, is version-gated, absent in older streams and taking values `0` and `1` when present. It is retained verbatim; its role is unresolved.

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
