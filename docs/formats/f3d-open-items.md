# Autodesk Fusion 360 `.f3d`: Open Items

This document lists the parts of the F3D format that we do not know. The specification `f3d.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Geometry carriers

### GC-01. `VBL_OFFSURF` and `skin_spl_sur2` payload layout

**Question.** What fields does a `VBL_OFFSURF` payload have? What fields does a `skin_spl_sur2` payload have?

**Known.** `VBL_OFFSURF` has the second spelling `offsetvbsur`. Most of these records include a final solved cache. The cache gives the exact face shape.

**Need.** Some records do not include a cache. For those records, the field layout is the only source of the face shape. We must know the layout to read them and to write them. The decoder keeps these records as opaque bytes now.

### GC-02. Standalone `offset` and `sur-sur-int` layout

**Question.** What fields does a standalone `offset` record have? What fields does a standalone `sur-sur-int` record have?

**Known.** The name `offset` is related to the `off_spl_sur` construction. The name `sur-sur-int` is related to the surface-to-surface intersection graph. Other records store that graph.

**Need.** We must know the field boundaries to read these two record names directly.

### GC-03. Second boolean of a cache-first `par_int_cur`

**Question.** What does the second boolean flag at the end of a cache-first `par_int_cur` record control?

**Known.** `f3d.md:467` gives the first flag. The first flag is the support-slot selector. The second flag takes the value `true` and the value `false`. Two records in one file can have different values. No other field changes with it.

**Need.** We keep this flag without a change. To make this record from a neutral model, we must know which value to write.

### GC-04. Single parameter curve in a cache-first `par_int_cur`

**Question.** Can a cache-first `par_int_cur` record store only one parameter-curve slot? If it can, what controls this?

**Known.** `f3d.md:467` gives two ordered parameter-curve slots. An absent curve uses the `nullbs` sentinel in its slot.

**Need.** A record with one slot moves all fields after it. The decoder then reads the record incorrectly.

**Conflict.** `f3d.md:461` and `f3d.md:467` give two parameter curves with no option. The decoder reads exactly two. This item says that records with one curve exist. Both statements cannot be correct. Decide which one is correct.

### GC-05. Second boolean of the `off_spl_sur` sense pair

**Question.** What does each boolean of the revision-gated `off_spl_sur` sense pair control? Which of the two mixed states gives which offset side?

**Known.** `f3d.md:491` gives the pair a joint role. The pair holds record-level progenitor sense state. It is not a per-axis decomposition. The pair takes the states false/false, true/false, and false/true. A false/false pair gives the offset side in the sign of the stored distance.

**Need.** We must know the individual roles to set the offset side correctly when we write this record.

**Note.** An earlier model gave the first boolean the offset-direction reversal role and gave the second boolean no effect. Commit `636c2c71d` withdrew that model. Do not use it.

### GC-06. `off_spl_sur` ASM extension prefix

**Question.** What do the two booleans of the `off_spl_sur` ASM extension prefix control?

**Known.** `f3d.md:491` gives the position of the prefix. The prefix comes after the sense pair and before the shared revision-gated surface tail.

**Need.** We must know which value to write for each boolean when we make this record.

### GC-07. `off_spl_sur` extension run

**Question.** What are the field roles of the extra run in an `off_spl_sur` record? Which extension-prefix flag controls the presence of this run?

**Known.** A record with two `true` extension-prefix flags has an extra run before the shared tail. The run holds one boolean, six integers, one boolean, an embedded cache-first intcurve with optional endpoints, more booleans, a small tolerance scalar, and four `-1` integers. The first fields of the run agree with the start of a surface-to-surface intersection payload. That start is a presence logical, a six-integer header, an intersection curve, two pcurve logicals, and a tolerance. The four `-1` integers at the end are more than the three endpoint-term slots of that layout.

**Need.** We keep these records without a change. We cannot read the run, and we cannot make a record that has one.

### GC-08. Shared revision-gated surface tail enum

**Question.** What does the enum at the start of the shared revision-gated surface tail select? What are its nonzero values? What layout does each nonzero value select?

**Known.** `f3d.md:489` gives the value zero. Zero selects the full solved-cache tail. A nonzero value has no known layout. The decoder keeps a record with a nonzero value as opaque bytes.

**Need.** We must know the nonzero layouts to read those records and to build a neutral model from them.

### GC-09. Law operators `MIN`, `MAX`, and `STEP`

**Question.** Does the framed law-expression layout accept the operators `MIN`, `MAX`, and `STEP`? If it accepts them, how many operands does each operator take? Where does each operand stop?

**Known.** These three operators have a variable number of operands. `STEP` is present in stored law formula strings. We keep those strings as text. They have no framed node, no child count, and no delimiter. `MIN` and `MAX` have no known framed form. `f3d.md:525` gives the fixed operand count of every other framed operator.

**Need.** The net, skin, and sweep payloads use framed law expressions. We must know the operand boundary to write a framed expression that uses these three operators.

### GC-10. Variable-blend side extension integer

**Question.** What does the integer between the secondary pcurve and the tertiary pcurve of a variable-blend support side control?

**Known.** `f3d.md:541` gives the position and the type of the integer. `f3d.md:658` gives its presence rule. A modern side has this integer and a tertiary pcurve. A legacy side stops after the secondary pcurve. The value does not change with the presence of the secondary pcurve.

**Need.** We keep this integer without a change. To make a variable-blend side from a neutral model, we must know which value to write.

### GC-11. U and V order of unextended intervals

**Question.** Which of the two unextended parameter intervals gives the U direction? Which gives the V direction? This applies to a revision-gated `exact_spl_sur` and to a revision-gated `t_spl_sur`.

**Known.** `f3d.md:481` gives two intervals in serialized order. Each interval gives the unextended range in one direction. The specification does not say which direction comes first.

**Need.** The neutral model stores a U range and a V range. A wrong order gives the surface a wrong parameter range.

**Conflict.** The comment at `crates/cadmpeg-codec-f3d/src/nurbs/proc_surface.rs:3341` says that the first interval is U and the second is V. The comment gives no source. Either prove the order and put it in `f3d.md`, or remove the claim from the comment.

### GC-12. Closure direction of `loft_spl_sur` wrap ranges

**Question.** Which of the two `loft_spl_sur` wrap-range intervals gives which closure direction of the solved surface?

**Known.** `f3d.md:499` gives the two intervals in serialized order. A non-empty interval shows that the solved surface is closed in one direction. The interval order does not agree with a rule that makes the first interval U.

**Need.** The neutral model records closure per direction. A wrong order closes the wrong direction.

### GC-13. `cl_loft_spl_sur` tail kind values

**Question.** What are the nonzero values of the revision-gated `cl_loft_spl_sur` tail-kind integer? What layout does each nonzero value select?

**Known.** `f3d.md:501` gives the kind-zero payload. The decoder rejects every nonzero kind.

**Need.** We cannot read a record with a nonzero kind.

### GC-14. `cl_loft_spl_sur` optional trailing BS3 curve

**Question.** What controls the presence of the trailing BS3 curve in the kind-zero `cl_loft_spl_sur` payload?

**Known.** `f3d.md:501` gives the kind-zero payload with an optional trailing BS3 curve. `f3d.md:489` gives the bool-gated encoding for the optional parameter values in the same payload. The decoder finds the trailing curve with a look-ahead for the subtype-close byte.

**Need.** The look-ahead rule is not in the specification. We must state the true presence rule to write this payload correctly.

### GC-15. First integer of a legacy variable-blend record

**Question.** Is there an older `var_blend_spl_sur` or `srf_srf_v_bl_spl_sur` layout in which the first integer is a subtype definition-table index and not the serializer revision?

**Known.** `f3d.md:541` says that the record starts with a serializer-revision integer. `f3d.md:612` gives the related record `rb_blend_spl_sur` a `LONG definition_index` in the same position.

**Need.** A wrong reading of the first integer moves every field after it.

**Conflict.** `f3d.md:541` states the serializer-revision integer as a fact for all layouts. This item questions that statement for older layouts. Either limit the specification sentence to the revision era, or remove this item.

### GC-16. Token tags of a revision-gated `VBL_SURF` `deg` boundary

**Question.** Which token tags does a revision-gated `VBL_SURF` `deg` boundary use for its location and for its two normals?

**Known.** `f3d.md:543` gives the revision-form tag changes for the type names, the magic location, the support bounds, the curve endpoints, and circle form `3`. It does not give the tags of a `deg` boundary. The decoder writes `0x13` and two `0x14` tokens.

**Need.** A wrong tag makes a file that Fusion cannot read.

### GC-17. Variable-blend tail integer values

**Question.** What are the values of the approximation-current integer other than `1`? What are the values of the count integer other than `1`? What fields does a count value more than `1` select?

**Known.** `f3d.md:541` gives the position and the type of both integers. The count integer is not negative.

**Need.** The decoder reads no counted payload now. A count value more than `1` adds fields that we do not read.

### GC-18. Blend-value selector values

**Question.** We must find four answers:

- the two-radii chamfer-selector values other than `0` and `3`
- the single-radius selector values other than `0`, `1`, and `7`
- the difference between single-radius selector `1` and single-radius selector `7`
- the meaning of the optional two-scalar `interp` tail

**Known.** `f3d.md:541` gives the layout for chamfer selectors `0` and `3`. It gives the layout for single-radius selectors `0`, `1`, and `7`. A trailing flag selects the `interp` tail.

**Need.** The decoder rejects every other selector value. We cannot read those records. Selector `1` and selector `7` select the same two scalars, so we cannot write the correct one from a neutral model.

### GC-19. First two `tvertex` tolerance evaluations

**Question.** What do the first two tolerance evaluations of a `tvertex` record hold? What controls the set state of each one?

**Known.** `f3d.md:350` gives the three slots. The last slot is always set and is the vertex tolerance. A `-1` slot shows an unset evaluation. Any leading slot can be unset. When slots two and three are both set, they do not decrease in slot order.

**Need.** We keep the two leading slots without a change. To write them from a neutral model, we must know what they hold.

### GC-20. Trailing LONG of `tedge` and `tvertex`

**Question.** What does the `tedge` trailing LONG (`chunk[13]`) control? What does the `tvertex` trailing LONG (`chunk[9]`) control? Do the two fields use the same version gate?

**Known.** `f3d.md:346` and `f3d.md:350` give the position, the type, the values `0` and `1`, and the version gate of each field.

**Need.** We keep both fields without a change. To write them from a neutral model, we must know which value to write. The decoder accepts each field when a `Long` token is present. No document proves that the two gates are the same.

## 2. Container, header, and design records

### DR-01. Header flag bits 1 and above

**Question.** What do bits 1 and above of the header flags word mean?

**Known.** `f3d.md:191` gives bit 0. Bit 0 marks a history partition in both widths. We keep the other bits without a change.

**Need.** We must know the bit meanings to set them correctly in a new header.

### DR-03. Second `0x01`-marker u32 of an ACT root-component link

**Question.** What does the second `0x01`-marker u32 of an ACT root-component link record control?

**Known.** `f3d.md:940` gives the byte sequence. It holds `0x01`, `u32 3`, five zero bytes, `0x01`, and `u32 registry_flag`. The value of `registry_flag` is `0` or `1`.

**Need.** We must know the meaning to write the correct value in a new record.

### DR-04. Configuration rules without paired members

**Question.** What is the activation layout of a configuration-rule object that has no paired string `when` member and `activate` member?

**Known.** `f3d.md:54` gives the paired form. The `when` member holds the activation condition. The `activate` member holds the target variant name. `f3d.md:56` keeps unrecognized members without a change.

**Need.** We cannot build the activation condition of a rule that uses another form.

### DR-05. Recipe records of a non-locus parameter companion

**Question.** How do the recipe records inside one non-locus indexed-parameter-companion variant relate to each other as an operation?

**Known.** `f3d.md:724` gives the containment order and the retention order. `f3d.md:726` gives the edge-recipe-subsequence join. `f3d.md:728` gives the measurement rule for a recipe-backed linear dimension that has no locus.

**Need.** We must know the operation to build a neutral dimension from more than one recipe record.

### DR-06. Sketch-relation member-role values

**Question.** What do the member-role values of a sketch relation mean?

**Known.** `f3d.md:912` gives two special cases. A `0x20000000000` relation uses the path role and the text role. A `0x2000000000` relation uses role `1` for result curves. The same line states that the role is relation-specific and does not classify an input member or a generated member by itself.

**Need.** We must know the general value set to classify members in every other relation.

### DR-07. Point-to-surface loci of roles `0` to `3`

**Question.** Which point-to-surface locus does each sketch-relation member-role value `0`, `1`, `2`, and `3` select?

**Known.** `f3d.md:934` states that the surface member role does not change the neutral point-on-surface relation. The four values do not select the four control-grid corners.

**Need.** We must know the locus to build the correct neutral constraint.

**Conflict.** `f3d.md:934` can mean that the role selects no locus at all. If that is the intent, this item is not valid and must be removed. Decide the intent of `f3d.md:934`.

### DR-08. Member roles of a spline-group relation

**Question.** What do the explicit per-member role integers of a `0x80000000` spline-group relation mean?

**Known.** `f3d.md:912` gives the member positions by structure. In a model-space spline group, member `i` joins control points `i` and `i+1`. This rule uses the member position and not the role integer.

**Need.** We must know the role meaning to write the correct integer for each member.

### DR-09. Sheet-metal `EdgeFlange` and `Hem` discriminators

**Question.** What do the values of these discriminators mean?

- the `EdgeFlange` extent discriminator
- the `EdgeFlange` height-datum discriminator
- the `EdgeFlange` bend-position discriminator
- the `Hem` direction discriminator
- the `Hem` hem-form discriminator

**Known.** `f3d.md:874` and `f3d.md:876` give the offset and the width of each field. The decoder keeps every one of them as an uninterpreted value.

**Need.** These two features have no neutral operation without the value meanings. We cannot rebuild the feature in a neutral model.

### DR-10. `SpirePrimitive` and `CoilPrimitive` values

**Question.** What do these values mean?

- `SpirePrimitive` section-placement values other than `4`
- `CoilPrimitive` operation values other than `1`
- `CoilPrimitive` extent values other than `1`
- `CoilPrimitive` section values other than `1`
- `CoilPrimitive` section-placement values other than `3`
- the fixed u32 value at primary-header offset 26 in each record

**Known.** `f3d.md:770` gives the `SpirePrimitive` fields. `f3d.md:772` gives the `CoilPrimitive` fields. Each field has one known value. The two records use different numbers for the same meaning, so a value from one record does not transfer to the other.

**Need.** We must know the value sets to build these two features in a neutral model.

### DR-11. Eighth `CoilPrimitive` reference

**Question.** What does the eighth ordered `CoilPrimitive` scope reference select?

**Known.** `f3d.md:772` gives seven of the eight references. The first two keep the placement construction. Five of the other six each control one compact parameter: `Diameter`, `SectionSize`, `TaperAngle`, `Revolutions`, and `Height`.

**Need.** We must know the eighth reference to keep the complete feature input set.

### DR-12. `EntityGenesis` placement field at offset 45

**Question.** What does the f64-shaped field that ends at primary-record offset 45 hold? This field is in the `EntityGenesis`-form placement record class.

**Known.** `f3d.md:900` gives this record from offset 55. It gives `0x01` at offset 55, nine zero bytes, and a form byte at offset 65. It gives no field before offset 55.

**Need.** We must know the field to write a complete placement record.

### DR-13. `WorkPoint` without an explicit coordinate

**Question.** Which construction-record join gives the position of a reference-derived `WorkPoint` that has no explicit class-282 coordinate?

**Known.** `f3d.md:906` gives the three forms that store a coordinate. They are the 197-byte, 208-byte, and 207-byte forms. The decoder accepts only those three forms. For any other form it makes a native feature with no position.

**Need.** The neutral model needs a position for every work point.

### DR-14. `SurfacePatch` patch-setting records

**Question.** What do the fields of the two patch-setting records hold? These records are at ordered reference positions two and three of the 354-byte `SurfacePatch` scope.

**Known.** `f3d.md:812` says that the remaining references carry patch settings. It gives no field meanings.

**Need.** We must know the settings to rebuild the patch in a neutral model.

### DR-15. Recipe fields for ambiguous edge operands

**Question.** Which recipe field assigns the active B-rep edge identity when the candidate set is empty, disjoint, or has more than one intersection?

**Known.** `f3d.md:838` gives the result of these cases. An empty leading reference set identifies an unresolved edge-bearing operand. A disjoint set does the same. `f3d.md:952` keeps an unresolved identity without a change.

**Need.** The neutral model needs one edge identity per operand.

### DR-16. Extrude face-recipe candidate discriminator

**Question.** Which field selects one active B-rep face candidate when two rules both fail?

**Known.** `f3d.md:852` gives the two rules that work. The first rule applies when every effective active candidate has a support mapping and every mapping names the same non-empty predecessor-face set. The second rule uses a counted-boundary predecessor set.

**Need.** The neutral model needs one face per recipe member.

### DR-17. Extrude selection unknowns

**Question.** We must find five answers:

- what an identity that is absent from history denotes
- which field separates two profile loops that meet at the same ordered persistent Sketch points
- which field selects one of several closed spatial-Sketch profiles
- what the context UUID names
- what the optional slot of the fixed member tail holds

**Known.** `f3d.md:844` states that an identity absent from the preceding state gives no candidate. `f3d.md:894` gives a fallback chain that ends in native retention. `f3d.md:890` gives the presence encoding of the optional slot. The marker is zero when the slot is absent and one when the slot is present.

**Need.** Each unknown makes one Extrude selection fall back to native retention. The neutral model then has no selection.

### DR-18. Shifted Extrude extent pairs

**Question.** What do the shifted Extrude extent-discriminator pairs other than `(1, 1)`, `(1, 2)`, `(2, 0)`, and `(3, 2)` mean?

**Known.** `f3d.md:796` gives those four pairs. They use the same meanings as the current prologue.

**Need.** We must know the other pairs to build the extent in a neutral model.

### DR-19. Construction-group fields

**Question.** What do the construction-group scalar fields hold? What does the variant byte control? What do the group-role values outside the defined feature-specific sets mean?

**Known.** `f3d.md:870` gives the positions and the value limits. The group holds a nonzero u32, a finite f64, a second copy of the u32, then `01`, a `variant` byte, and a zero byte. The value of `variant` is zero or one. `f3d.md:870` defines the Extrude roles `0x08`, `0x41`, and `0x11` only.

**Need.** We must know the field meanings to write a construction group from a neutral model. The role value `0x0000000500000000` in an Extrude scope is one case of an undefined role.

### DR-20. Face-recipe node scalar fields

**Question.** What is the topology meaning of the root scalar, the prelude scalars, and the side-clause scalars of a face-recipe node?

**Known.** `f3d.md:852` gives the node structure. The payload is a `-1`-delimited root scalar, two one-word prelude runs, and two topology side clauses. The root scalar, the first prelude scalar, and every side scalar keep their source field order and their repetitions. `f3d.md:854` states that the delimiter value does not change the retained field value or the side structure. `f3d.md:850` gives a role to these scalars in an **edge** recipe only.

**Need.** We must know the meaning to build face topology from a recipe without the edge-recipe rule.

### DR-21. Body identity of a `Move` or `RemoveBody` group

**Question.** Which join connects a `Move` or `RemoveBody` construction-group identity with role `0x0000000400000000` to a neutral body identity?

**Known.** `f3d.md:788` and `f3d.md:790` give the record layout.

**Need.** Without the join, the neutral model has no body selection for these two features.

### DR-22. Form-33 `Combine` body recipe

**Question.** Which field selects the input body for a form-33 `Combine` body-recipe identity that intersects more than one input body and has no matching occurrence elsewhere in the Design stream?

**Known.** `f3d.md:774` gives the complete persistent identity and the agreement rule. The identity is the stream GUID, the asset GUID, the context GUID, the ordered `(Design reference, form)` clauses, the recipe Design id, and the recipe selector. Repeated occurrences of that identity select the same stable history body when every resolved occurrence agrees.

**Need.** This item is the case in which the agreement rule has no input. The neutral model then has no body selection.

### DR-23. `Draft` direction fields

**Question.** How do the signed angle, the neutral-plane orientation, the explicit pull direction, and the outward-material convention of a `Draft` scope relate to each other?

**Known.** `f3d.md:776` gives the field roles. The first scalar is the nonzero signed draft angle in radians. Another field selects the neutral plane.

**Need.** The pull direction and the outward flag are redundant with the angle sign and the plane. We must know the relation to compute them. The neutral model leaves both empty now.

### DR-24. Class-365 whole-body operand fields

**Question.** What do the class-365 whole-body operand fields after the asset UUID and the context UUID hold? This question excludes the bounded nested-record join and the body-recipe join.

**Known.** `f3d.md:784` gives the reference count, the ordered `(Design reference, form)` pairs, the asset UUID, the context UUID, the `u32 2`, the four zero bytes, the paired header, and the nested indexed headers. It gives the byte layout of these fields and no meaning for the `u32 2` and the four zero bytes.

**Need.** We must know the field meanings to write a complete class-365 member.

### DR-25. Base Feature six-byte fields

**Question.** What do the six-byte fields after the Base Feature body suffixes and after the Base Feature record references hold?

**Known.** `f3d.md:780` states that every value has a six-byte trailing field. The decoder keeps each field as six opaque bytes.

**Need.** We must know the meaning to write a Base Feature from a neutral model.

### DR-26. Sketch-text fields

**Question.** What do these sketch-text fields hold?

- the f64 and the two f32 values between the nominal height and the font family
- the two internal record references
- the class-specific tail fields

**Known.** `f3d.md:914` gives the layout and names four fields. They are the nominal text height in centimetres, the width factor, the font family name, and the text string. The other fields have an offset and no meaning.

**Need.** We must know the meanings to write sketch text from a neutral model.

## 3. External references

### XR-01. `neutronData` with a different GUID

**Question.** What does `neutronData` mean when its GUID is different from the `neutronRole` GUID?

**Known.** `f3d.md:72` states that `neutronData` is an independent property. It does not need to equal `neutronRole`. The decoder keeps the value as a note string.

**Need.** We must know the meaning to build the external reference in a neutral model.

### XR-02. Non-empty `ComponentReferenceData.json`

**Question.** Which member names occur in a non-empty `ComponentReferenceData.json` object? What does each member control?

**Known.** `f3d.md:74` states that the object is an open document. It keeps member names and values without a closed schema. The decoder checks the envelope and makes no neutral projection.

**Need.** We must know the member names and their meanings to build component references in a neutral model.

### XR-03. `DcXRefPCIFeature` eight-byte value

**Question.** What does the `0x01`-tagged eight-byte value before the owning-design GUID hold? This value is in a `DcXRefPCIFeature` record.

**Known.** `f3d.md:76` gives the field order. The record holds marker-tagged integer fields, then this eight-byte value, then a `u32`-count NUL-terminated ASCII GUID. The GUID names the owning design object.

**Need.** We must know the meaning to write this record from a neutral model.

### XR-04. Occurrence-placement tail u32 fields

**Question.** What do the two u32 fields of the role-adjacent occurrence-placement tail hold?

**Known.** `f3d.md:78` gives the layout. The tail starts after the occurrence-role string with `u8 0` and one u32. Then it has zero or more entries. Each entry holds `u8 1`, a u64 record reference, two zero bytes, and a u32. The decoder skips both u32 fields.

**Need.** We must know the meanings to write a complete occurrence placement.

## 4. Material assets

### MA-01. TextureURI leading u8

**Question.** What does the u8 before a TextureURI path count hold?

**Known.** `f3d.md:982` gives the layout. TextureURI stores one u8, then a little-endian u32 path count. The decoder discards the u8.

**Need.** We must know the meaning to write a texture URI from a neutral model.

### MA-02. `texture_RealWorldOffsetX` prelude

**Question.** What does the u32 prelude before `texture_RealWorldOffsetX` hold?

**Known.** `f3d.md:986` states that one u32 prelude comes before this property. It gives no meaning.

**Need.** We must know the meaning to write the texture offset from a neutral model.

### MA-03. Distance unit-tag values

**Question.** What are the unit tags of a Distance value other than the inch tag and the centimetre tag?

**Known.** `f3d.md:982` gives two tags. Tag `0x2016` is inches. Tag `0x200e` is centimetres. The decoder returns no unit for every other tag.

**Need.** A Distance with an unknown tag gets no unit. The neutral model then has a value with no scale.

### MA-04. Texture map-channel values

**Question.** What does each texture map-channel integer value mean?

**Known.** `f3d.md:986` names the map-channel property. The application defines the value meanings.

**Need.** We must know the meanings to map a texture to the correct channel in a neutral model.

### MA-05. Canvas visibility, mirroring, and crop

**Question.** Which Canvas fields hold the visibility state, the mirroring state, and the crop state?

**Known.** `f3d.md:754` gives the Canvas geometry record. It holds the opacity, the plane frame, both boundary segments, the label, and the image asset. It names no visibility field, no mirroring field, and no crop field. The decoder reads the opacity and the plane frame only.

**Need.** A neutral canvas needs these three states to show the image correctly.

### MA-06. External material-library payloads

**Question.** What are the appearance properties behind each material-library preset phrase?

**Known.** `f3d.md:1012` states that library display names are not in the file. They resolve through the external material library. The preset phrase is the key.

**Need.** Without the library, a preset phrase gives no concrete appearance. The neutral model then has no material.

**Note.** This item needs a file outside the `.f3d` container. We cannot close it from the container alone.

### MA-07. Precedence of library colour records

**Question.** What is the precedence of the `color-adesk-attrib` record and the `material-adesk-attrib` record against direct colours and appearance assignments?

**Known.** `f3d.md:974` gives the content of both records. `color-adesk-attrib` holds a palette index. `material-adesk-attrib` holds a library lookup pair. `f3d.md:994` gives the precedence of the two other colour records only. An explicit `rgb_color-st-attrib` or `truecolor-adesk-attrib` on a body or a face gives that target its neutral colour. If neither is present, one appearance binding with a base colour gives the colour.

**Need.** A target can have more than one colour source. We must know the order to select one neutral colour.

## 5. T-splines

### TS-01. `0m cg` derived-grip fields

**Question.** What do the individual scalar values and index values inside a `0m cg` derived-grip record hold?

**Known.** `f3d.md:36` gives the record category. A `0m cg` record describes derived grip connectivity. It does not replace the primary topology map. The decoder discards the record fields.

**Need.** We must know the field meanings to keep grip connectivity in a neutral model.

### TS-02. Cage-object join for more than one Form scope

**Question.** Which cage-object identity join divides the active TSM entries between two or more Form scopes?

**Known.** `f3d.md:40` gives the single-Form case. One Form whose list count equals every active-folder TSM entry owns those cages in archive order.

**Need.** With two or more Form scopes, we cannot give each cage to the correct Form.
