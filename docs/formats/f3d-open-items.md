# Autodesk Fusion 360 `.f3d`: Open Items

This document lists the parts of the F3D format that we do not know. The specification `f3d.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

A reference to the specification gives the section number and the start of the paragraph. An example is `f3d.md` §7.3 `off_spl_sur`. Do not use line numbers. Line numbers become incorrect when the specification changes. The `scripts/check-doc-anchors.py` command makes sure that each reference finds exactly one paragraph.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Geometry carriers

### GC-01. `VBL_OFFSURF` and `skin_spl_sur2` payload layout

**Question.** What fields does a `VBL_OFFSURF` payload have? What fields does a `skin_spl_sur2` payload have?

**Known.** `VBL_OFFSURF` has the second spelling `offsetvbsur`. Most of these records include a final solved cache. The cache gives the exact face shape.

**Need.** Some records do not include a cache. For those records, the field layout is the only source of the face shape. We must know the layout to read them and to write them. The decoder keeps these records as opaque bytes now.

### GC-05. Second boolean of the `off_spl_sur` sense pair

**Question.** What does the second boolean of the revision-gated `off_spl_sur` sense pair control?

**Known.** `f3d.md` §7.3 `off_spl_sur` gives the first boolean and the offset construction. The second boolean does not select the offset side and does not move the offset surface. It is set only in records whose first boolean is also set, and those records have a support surface whose stored parameterization is reflected relative to the model.

**Need.** We must know which value to write when we make this record from a neutral model. `false` is the value for a surface built directly from a support surface and a distance, so the item blocks only the reflected case.

### GC-06. `off_spl_sur` ASM extension prefix

**Question.** What do the two booleans of the `off_spl_sur` ASM extension prefix control?

**Known.** `f3d.md` §7.3 `off_spl_sur` gives the position of the prefix. The prefix comes after the sense pair and before the shared revision-gated surface tail. The prefix takes the state false/false. No other state has a known occurrence.

**Need.** We must know which value to write for each boolean when we make this record.

### GC-07. `off_spl_sur` extension run

**Question.** What are the field roles of the extra run in an `off_spl_sur` record? Which extension-prefix flag controls the presence of this run?

**Known.** A record with two `true` extension-prefix flags has an extra run before the shared tail. The run holds one boolean, six integers, one boolean, an embedded cache-first intcurve with optional endpoints, more booleans, a small tolerance scalar, and four `-1` integers. The first fields of the run agree with the start of a surface-to-surface intersection payload. That start is a presence logical, a six-integer header, an intersection curve, two pcurve logicals, and a tolerance. The four `-1` integers at the end are more than the three endpoint-term slots of that layout.

**Need.** We keep these records without a change. We cannot read the run, and we cannot make a record that has one.

### GC-08. Shared revision-gated surface tail enum values other than `0` and `2`

**Question.** What layouts do the shared revision-gated surface tail enum values other than `0` and `2` select?

**Known.** `f3d.md` §7.3 "**Revision-gated spline-surface forms**" gives the layouts of `0` and `2`. Zero selects the solved cache and its fit tolerance; two replaces both with the two bool-gated parameter intervals and four closure and singularity enums. The decoder keeps a record with any other value as opaque bytes.

**Need.** We cannot read a record with a value other than `0` or `2`.

### GC-13. `cl_loft_spl_sur` tail kind values

**Question.** What are the nonzero values of the revision-gated `cl_loft_spl_sur` tail-kind integer? What layout does each nonzero value select?

**Known.** `f3d.md` §7.3 `cl_loft_spl_sur` gives the kind-zero payload. The revision-gated form takes tail kind `0`; no nonzero kind has a known occurrence in that form. The decoder rejects every nonzero kind.

**Need.** We cannot read a record with a nonzero kind.

### GC-14. `cl_loft_spl_sur` optional trailing BS3 curve

**Question.** What controls the presence of the trailing BS3 curve in the kind-zero `cl_loft_spl_sur` payload?

**Known.** `f3d.md` §7.3 `cl_loft_spl_sur` gives the kind-zero payload with an optional trailing BS3 curve. `f3d.md` §7.3 "**Revision-gated spline-surface forms**" gives the bool-gated encoding for the optional parameter values in the same payload. The trailing curve is present exactly when the two optional parameter values are present in their bool-gated encoding; the present pair can be the descending no-range sentinel, so the rule couples to the encoding and not to the numeric values. The decoder finds the trailing curve with a look-ahead for the subtype-close byte and keeps that look-ahead until the rule is confirmed more widely. Every observed kind-zero payload ends in two absent-value booleans followed directly by the subtype close, which is byte-identical to two plain flags and no trailing-curve slot at all, so the corpus separates neither the two readings of those booleans nor the look-ahead from the stated rule.

**Need.** The look-ahead rule is not in the specification. We must state the true presence rule to write this payload correctly.

### GC-16. Token tags of a revision-gated `VBL_SURF` `deg` boundary

**Question.** Which token tags does a revision-gated `VBL_SURF` `deg` boundary use for its location and for its two normals?

**Known.** `f3d.md` §7.3 `VBL_SURF` gives the revision-form tag changes for the type names, the magic location, the support bounds, the curve endpoints, and circle form `3`. It does not give the tags of a `deg` boundary. The decoder writes `0x13` and two `0x14` tokens.

**Need.** A wrong tag makes a file that Fusion cannot read.

### GC-17. Variable-blend approximation-current integer values

**Question.** What does the approximation-current integer mean, and which value must a writer emit?

**Known.** `f3d.md` §7.3 `var_blend_spl_sur` gives the position and the type of the integer, and gives the signed handedness marker that follows the fit tolerances. The integer takes only `0` and `1`. It selects no layout: the two fit tolerances, the handedness marker, and the cache-selector enum follow it unchanged under both values, and no later field reads it. It does not control whether a solved cache is stored — the cache-selector enum does, and the two dissociate in both directions.

**Need.** We must know which value to write from a neutral model.

### GC-18. Blend-value selector values

**Question.** We must find three answers:

- the two-radii chamfer-selector values other than `0` and `3`
- the single-radius selector values other than `0`, `1`, and `7`
- the difference between single-radius selector `1` and single-radius selector `7`

**Known.** `f3d.md` §7.3 `var_blend_spl_sur` gives the layout for chamfer selectors `0` and `3`. It gives the layout for single-radius selectors `0`, `1`, and `7`. Selector `0` carries a rational cache whose cross-section is an exact circular arc; selector `7` carries a non-rational cache whose cross-section is not a circular arc, so the two scalars do not enter the two selectors the same way.

**Need.** The decoder rejects every other selector value. We cannot read those records. Selector `1` and selector `7` select the same two scalars, so we cannot write the correct one from a neutral model.

### GC-19. First `tvertex` tolerance evaluation

**Question.** What does the first tolerance evaluation of a `tvertex` record hold, and what controls its set state?

**Known.** `f3d.md` §6.2 "**Tolerant vertex:**" gives the three slots, the second slot's construction from the incident edge-endpoint gaps and tolerant-edge tolerances, the third slot's raise over the incident edges that are not tolerant, and the set-state relation between the second and third slots. The first slot is unset in every record a current writer emits, so its content and its set condition are undetermined. The predicate that selects which incident edges contribute the third slot's `1e-6` raise is narrower than "every edge that is not tolerant": some records carry a third slot equal to the second where that rule predicts a raise.

**Need.** To write the first slot from a neutral model we must know which length it measures and when a writer must set it. To write the third slot exactly we must know the contribution predicate.

### GC-21. Revision-gated `loft_spl_sur` type-zero member and ASM-integer gate

**Question.** What do the two nullable spline slots of a type-zero profile member hold, and are they BS2 pcurves or BS3 curves? What form does a type-zero member take in a save-format-23200 stream? At which save format version does the ASM integer start?

**Known.** `f3d.md` §7.3 `loft_spl_sur` gives the two member forms and the save-format gate of the ASM integer. A type-zero member stores two nullable spline slots in place of the support surface and the first flag. The decoder keeps both slots and reads them as BS2 pcurves. Every observed type-zero slot is the null-spline sentinel, and that sentinel is the same token for a BS2 and a BS3 slot, so the slot type is undetermined: only a type-zero member with a non-null slot separates them, by the count of scalars per control point. No type-zero member occurs in a save-format-23200 stream. No stream with a save format version between 22600 and 23200 holds a revision-gated loft. The decoder reads the ASM integer in each stream with a save format version above 22600.

**Need.** We keep the two slots without a change. To write them from a neutral model, we must know what they hold. To write a type-zero member into a save-format-23200 stream, we must know if that stream keeps the ASM integer. To write a stream with a save format version between 22600 and 23200, we must know if that stream keeps the ASM integer.

### GC-23. Cache-first intcurve leading enum values other than `0` and `2`

**Question.** What layouts do the cache-first intcurve leading enum values other than `0` and `2` select?

**Known.** `f3d.md` §7.3 "**Cache-first subtype selection**" gives the layouts of `0` and `2`. The decoder retains a record with a nonzero value verbatim, including value `2`, whose layout is now defined but not yet decoded.

**Need.** We cannot read a record with a value other than `0`.

### GC-24. Binding of the law formula text infix operator `O`

**Question.** What are the precedence and associativity of the infix operator `O` in stored law formula text?

**Known.** `f3d.md` §7.3 "**Law formulas**" gives `O` as composition with the right operand innermost, and gives the `MTRAIL` curve as a rail direction requiring no further construction input. A writer parenthesizes both `O` operands, so stored text never exercises the operator's binding against a neighbouring operator and never chains two occurrences.

**Need.** We must know the binding to parse law text that a different writer produced without full parenthesization. Text this codec emits is unaffected, because it parenthesizes both operands.

## 2. Container, header, and design records

### DR-03. ACT table trailing GUID run

**Question.** What does the run of LP-UTF16 GUIDs after an ACT table's counted entry list hold? What gives its length?

**Known.** `f3d.md` §8.1 "**The ACT segment.**" gives the two-byte prologue, the counted `(reference, entity key)` entries, and the join from each entry to its change group. The run after the last entry is a sequence of 36-character GUID strings and has no count of its own.

**Need.** We must know the run to write an ACT table. A reader that stops after the counted entries keeps every change-version join, so the item blocks writing only.

### DR-05. Recipe records of a non-locus parameter companion

**Question.** How do the recipe records inside one non-locus indexed-parameter-companion variant relate to each other as an operation?

**Known.** `f3d.md` §8.1 "Within a dimensional companion," gives the containment order and the retention order. `f3d.md` §8.1 "An edge recipe's words" gives the edge-recipe-subsequence join. `f3d.md` §8.1 "A recipe-backed linear dimension" gives the measurement rule for a recipe-backed linear dimension that has no locus.

**Need.** We must know the operation to build a neutral dimension from more than one recipe record.

### DR-09. Sheet-metal `EdgeFlange` and `Hem` discriminators

**Question.** What do the values of these discriminators mean?

- the `EdgeFlange` extent discriminator
- the `EdgeFlange` height-datum discriminator
- the `EdgeFlange` bend-position discriminator
- the `Hem` direction discriminator
- the `Hem` hem-form discriminator

**Known.** `f3d.md` §8.1 "An `EdgeFlange` scope with" and `f3d.md` §8.1 "A `Hem` scope has" give the offset and the width of each field. The decoder keeps every one of them as an uninterpreted value.

**Need.** These two features have no neutral operation without the value meanings. We cannot rebuild the feature in a neutral model. **Blocked on a specimen:** no sheet-metal scope of any family is available to read. Settling it needs a base face, one edge flange cycled through each hem form, one flange whose height is measured to a plane and one to a point, and one flange per width mode and per bend position.

### DR-10. `SpirePrimitive` and `CoilPrimitive` values

**Question.** What do these values mean?

- `SpirePrimitive` section-placement values other than `4`
- `CoilPrimitive` operation values other than `1`
- `CoilPrimitive` extent values other than `1`
- `CoilPrimitive` section values other than `1`
- `CoilPrimitive` section-placement values other than `3`
- the fixed u32 value at primary-header offset 26 in each record

**Known.** `f3d.md` §8.1 "`SpirePrimitive` selects the Coil" gives the `SpirePrimitive` fields. `f3d.md` §8.1 "`CoilPrimitive` is the compact" gives the `CoilPrimitive` fields. Each field has one known value. The two records use different numbers for the same meaning, so a value from one record does not transfer to the other. The ten-reference `CoilPrimitive` form's member layout is settled and its section placement precedes its section shape in the stream, which is the opposite of the order the compact form's offsets assume.

**Need.** We must know the value sets to build these two features in a neutral model. **Blocked on specimens:** every discriminator carries one value in the records available. Settling it needs one coil per creation type, one per boolean operation, and a grid of section shape against section placement, one coil per file. The compact form is absent, so whether it is the same class as the ten-reference form is also open.

### DR-11. Eighth `CoilPrimitive` reference

**Question.** Which entity does the member identity of the eighth ordered `CoilPrimitive` reference select? What is the layout of the larger ten-reference `CoilPrimitive` form?

**Known.** `f3d.md` §8.1 "`CoilPrimitive` is the compact" gives all eight references of the 427-byte form. The eighth is a counted selection group with one persistently identified member. It is not a compact parameter: the scope owns exactly five parameters, and the five parameter references name all of them. A larger `CoilPrimitive` form has a 573-byte frame and ten ordered references with no known layout.

**Need.** We must know the selected entity to keep the complete feature input set. The eighth ordered reference is the tool body in the compact form only; in the ten-reference form the tool body is the tenth and the eighth is a parameter.

### DR-12. Placement `refType` values

**Question.** What construction does each `refType` value of the placement class select?

**Known.** `f3d.md` §8.1 "A `Sketch` scope joined" gives the member order, the version gates, and the identity-marked matrix. `refType` is a u32 whose value does not change the member sequence, and the reference members it selects between — `rOrigin`, `rXAxis`, `rZAxis`, `rEdgeReferences`, `rPlaneReferences`, and `values` — are present at every value.

**Need.** We must know which references a given `refType` requires to write a placement that Fusion re-solves rather than accepts as stored.

### DR-13. `WorkPoint` `refType` values

**Question.** What construction does each `refType` value of the point-data class select?

**Known.** `f3d.md` §8.1 "A direct `WorkPoint` scope" gives the member order, the version gates, and the input count each `refType` implies. The stored `point3d` is the solved position for every value, so a reader needs no join to place the point.

**Need.** A writer must emit the `refType` that matches the inputs it writes, and a neutral model that edits an input must know which rule re-solves the point.

### DR-14. `SurfacePatch` boundary-settings values

**Question.** What do the values of `PatchContinuity` and `PatchFlip` mean?

**Known.** `f3d.md` §8.1 "A surface-patch boundary-settings record's" gives the member order and the types. `PatchContinuity` and `PatchFlip` are u32 and `PatchScale` is an f64.

**Need.** We must know the value sets to rebuild the patch in a neutral model. **Blocked on specimens:** `PatchContinuity` carries one value and `PatchFlip` two in the records available. Settling it needs one patch per continuity setting on a single boundary component, and one pair of patches that differ only in the boundary side.

### DR-15. Recipe fields for ambiguous edge operands

**Question.** Which recipe field assigns the active B-rep edge identity when the candidate set is empty, disjoint, or has more than one intersection?

**Known.** `f3d.md` §8.1 "An edge operand has" gives the result of these cases. An empty leading reference set identifies an unresolved edge-bearing operand. A disjoint set does the same. `f3d.md` §8.1 "For each selector, an" keeps an unresolved identity without a change.

**Need.** The neutral model needs one edge identity per operand.

### DR-16. Extrude face-recipe candidate discriminator

**Question.** Which field selects one active B-rep face candidate when two rules both fail?

**Known.** `f3d.md` §8.1 "An `Extrude` face-group member" gives the two rules that work. The first rule applies when every effective active candidate has a support mapping and every mapping names the same non-empty predecessor-face set. The second rule uses a counted-boundary predecessor set.

**Need.** The neutral model needs one face per recipe member.

### DR-17. Extrude selection unknowns

**Question.** We must find five answers:

- what an identity that is absent from history denotes
- which field separates two profile loops that meet at the same ordered persistent Sketch points
- which field selects one of several closed spatial-Sketch profiles
- what the context UUID names
- what the optional slot of the fixed member tail holds

**Known.** `f3d.md` §8.1 "A nested entity-selection member" states that an identity absent from the preceding state gives no candidate. `f3d.md` §8.1 "An Extrude selection resolves" gives a fallback chain that ends in native retention. `f3d.md` §8.1 "The construction-operand group's `identity_record_index`" gives the presence encoding of the optional slot. The marker is zero when the slot is absent and one when the slot is present.

**Need.** Each unknown makes one Extrude selection fall back to native retention. The neutral model then has no selection.

### DR-18. Extrude extent arbitration

**Question.** Which field determines the extent form when an extent discriminator and the stored termination reference disagree?

**Known.** `f3d.md` §8.1 "The extent form is carried by" gives the two per-side discriminators, their enum, and the parameter and reference set each value implies. Every implication holds in both directions, so no record separates the discriminator from the reference: the two never disagree. Extent value `3` does not occur, so neither its termination-entity search nor the tool-body extension mode of value `4` is exercised.

**Need.** A writer needs to know which field a reader follows before it can emit a record where the two differ, and whether value `3` may be written without a termination reference. A design authored with a to-object termination whose side is then switched to `to next` without clearing the object settles it.

### DR-19. Construction-group fields

**Question.** What do the construction-group scalar fields hold? What does the variant byte control? What do the group-role values outside the defined feature-specific sets mean?

**Known.** `f3d.md` §8.1 "Every `Extrude`, `Extrusion`, `Fillet`," gives the positions and the value limits. The group holds a nonzero u32, a finite f64, a second copy of the u32, then `01`, a `variant` byte, and a zero byte. The value of `variant` is zero or one. The same paragraph defines the Extrude roles `0x08`, `0x41`, and `0x11` only. The f64 is not equal to a compact-parameter value in the same feature scope, with or without unit scaling. The u32 is below 256, has one value for all groups of one feature scope, and does not decrease with the record index. A `variant` value of one occurs only on a scope that has no history state.

**Need.** We must know the field meanings to write a construction group from a neutral model. The role value `0x0000000500000000` in an Extrude scope is one case of an undefined role.

### DR-20. Face-recipe node scalar fields

**Question.** What is the topology meaning of the root scalar, the prelude scalars, and the side-clause scalars of a face-recipe node?

**Known.** `f3d.md` §8.1 "An `Extrude` face-group member" gives the node structure. The payload is a `-1`-delimited root scalar, two one-word prelude runs, and two topology side clauses. The root scalar, the first prelude scalar, and every side scalar keep their source field order and their repetitions. `f3d.md` §8.1 "The face-node `-1`-delimited grammar" states that the delimiter value does not change the retained field value or the side structure. `f3d.md` §8.1 "The structured edge-recipe program" gives a role to these scalars in an **edge** recipe only.

**Need.** We must know the meaning to build face topology from a recipe without the edge-recipe rule.

### DR-21. Body identity of a `Move` or `RemoveBody` group

**Question.** Which join connects a `Move` or `RemoveBody` construction-group identity with role `0x0000000400000000` to a neutral body identity?

**Known.** `f3d.md` §8.1 "A `Move` scope references" and `f3d.md` §8.1 "A `RemoveBody` scope references" give the record layout.

**Need.** Without the join, the neutral model has no body selection for these two features.

### DR-22. Form-33 `Combine` body recipe

**Question.** Which field selects the input body for a form-33 `Combine` body-recipe identity that intersects more than one input body and has no matching occurrence elsewhere in the Design stream?

**Known.** `f3d.md` §8.1 "A `Combine` scope stores" gives the complete persistent identity and the agreement rule. The identity is the stream GUID, the asset GUID, the context GUID, the ordered `(Design reference, form)` clauses, the recipe Design id, and the recipe selector. Repeated occurrences of that identity select the same stable history body when every resolved occurrence agrees.

**Need.** This item is the case in which the agreement rule has no input. The neutral model then has no body selection.

### DR-23. `Draft` direction fields

**Question.** How do the signed angle, the neutral-plane orientation, the explicit pull direction, and the outward-material convention of a `Draft` scope relate to each other?

**Known.** `f3d.md` §8.1 "A `Draft` scope has" gives the field roles. The first scalar is the nonzero signed draft angle in radians. Another field selects the neutral plane.

**Need.** The pull direction and the outward flag are redundant with the angle sign and the plane. We must know the relation to compute them. The neutral model leaves both empty now. **Blocked on a specimen:** no `Draft` scope is available to read. Settling it needs a design holding a neutral-plane draft and a parting-line draft, plus a flip-flag and negative-angle pair authored to identical geometry.

### DR-24. Class-365 whole-body operand fields

**Question.** What do the class-365 whole-body operand fields after the asset UUID and the context UUID hold? This question excludes the bounded nested-record join and the body-recipe join.

**Known.** `f3d.md` §8.1 "A class-365 whole-body member" gives the reference count, the ordered `(Design reference, form)` pairs, the asset UUID, the context UUID, the `u32 2`, the four zero bytes, the paired header, and the nested indexed headers. The `u32 2` is a literal that never varies. The four bytes after it are not always zero, and the u32 after those takes a broad range of small values. The `0x01`-tagged value before the two UUIDs is a reference whose target is the containing entity plus three, so the two zero bytes after it are that reference's flags.

**Need.** We must know what the two variable u32 select to write a complete class-365 member.

### DR-25. Base Feature six-byte fields

**Question.** What do the six-byte fields after the Base Feature body suffixes and after the Base Feature record references hold?

**Known.** `f3d.md` §8.1 "**References.**" resolves the six bytes that follow an eleven-byte element: they are the target entity ID's high half and the two reference flags. A fifteen-byte element already carries the full entity ID, so its six bytes are instead the two flags and a further u32 member of the element. The `u16 0` then `u32 1` form is that member holding `1`: the flags are both clear, and a cross-segment reference would put `1` in the second flag byte rather than in the u32. The value is uniform across a scope's body-entity run.

**Need.** We must know what the per-element u32 selects to write a Base Feature from a neutral model. It is `1` on one scope and `0` on every other, and it does not track the body count, so we cannot say what selects it.

### DR-26. Sketch-text fields

**Question.** What do these sketch-text fields hold?

- the f64 and the two f32 values between the nominal height and the font family
- the two internal record references
- the class-specific tail fields

**Known.** `f3d.md` §8.1 "A sketch-text record has" gives the layout and names four fields. They are the nominal text height in centimetres, the width factor, the font family name, and the text string. The other fields have an offset and no meaning.

**Need.** We must know the meanings to write sketch text from a neutral model.

## 3. External references

### XR-01. `neutronData` with a different GUID

**Question.** What does `neutronData` mean when its GUID is different from the `neutronRole` GUID?

**Known.** `f3d.md` §1.4 `RedirectionsStream.dat` states that `neutronData` is an independent property. It does not need to equal `neutronRole`. The decoder keeps the value as a note string.

**Need.** We must know the meaning to build the external reference in a neutral model.

### XR-02. Non-empty `ComponentReferenceData.json`

**Question.** Which member names occur in a non-empty `ComponentReferenceData.json` object? What does each member control?

**Known.** `f3d.md` §1.4 `ComponentReferenceData.json` states that the object is an open document. It keeps member names and values without a closed schema. The decoder checks the envelope and makes no neutral projection.

**Need.** We must know the member names and their meanings to build component references in a neutral model.

### XR-03. Cross-document reference asset GUID

**Question.** What does the LP-UTF16 asset GUID of a cross-document reference name?

**Known.** `f3d.md` §8.1 "**References.**" gives the cross-document form. The eight-byte value the item once asked about is the reference's target entity ID, and the ASCII GUID after it is the target record's type GUID; both resolve against the segment the reference's segment ID names in the target document. The UTF-16 asset GUID between them is one value per link and does not equal the target segment's own asset GUID.

**Need.** A writer must emit this GUID to build a cross-document reference, and we cannot derive it from either end of the link.

### XR-04. Occurrence-placement reference runs

**Question.** What do the counted reference run after the matrix, the modern tagged u32 run, and the two closing references of an occurrence placement name?

**Known.** `f3d.md` §1.4 "**Placement.**" gives the layout, the instance discriminator, and the identity-marked matrix. The counted run reaches both local and cross-document targets. The two closing references name the same pair of entities for every placement of one document, so neither depends on the placement.

**Need.** We must know the targets to write a complete occurrence placement. A reader takes the target path, the discriminators, and the transform without them.

## 4. Material assets

### MA-03. Distance unit-tag values

**Question.** What are the unit tags of a Distance value other than the inch tag and the centimetre tag?

**Known.** `f3d.md` §8.2 "Boolean stores one u8." gives two tags. Tag `0x2016` is inches. Tag `0x200e` is centimetres. The decoder returns no unit for every other tag. The tag is a property of the asset and does not track the document display length unit, so varying that unit does not enumerate further tags. The schema `unit` attribute of a Distance property does not predict the tag: a property declared in millimetres serializes with the centimetre tag.

**Need.** A Distance with an unknown tag gets no unit. The neutral model then has a value with no scale. Further tags require an asset that stores a Distance in a unit other than inches or centimetres; the appearance library reached from a Fusion document stores every Distance with the centimetre tag.

### MA-04. Texture map-channel values

**Question.** What does each texture map-channel integer value mean?

**Known.** `f3d.md` §8.2 "`UnifiedBitmapSchema` and `BumpMapSchema` records" names the map-channel property. The application defines the value meanings.

**Need.** We must know the meanings to map a texture to the correct channel in a neutral model.

### MA-05. Canvas visibility, mirroring, and crop

**Question.** Which Canvas fields hold the visibility state, the mirroring state, and the crop state?

**Known.** `f3d.md` §8.1 "A `Canvas` scope names" gives the Canvas geometry record. It holds the opacity, the plane frame, both boundary segments, the label, and the image asset. It names no visibility field, no mirroring field, and no crop field. The decoder reads the opacity and the plane frame only.

**Need.** A neutral canvas needs these three states to show the image correctly.

### MA-06. External material-library payloads

**Question.** What are the appearance properties behind each material-library preset phrase?

**Known.** `f3d.md` §8.2 "Material records store a" states that library display names are not in the file. They resolve through the external material library. The preset phrase is the key.

**Need.** Without the library, a preset phrase gives no concrete appearance. The neutral model then has no material.

**Note.** This item needs a file outside the `.f3d` container. We cannot close it from the container alone.

### MA-07. Precedence of library colour records

**Question.** What is the precedence of the `color-adesk-attrib` record and the `material-adesk-attrib` record against direct colours and appearance assignments?

**Known.** `f3d.md` §8.2 "Color attribute records include" gives the content of both records. `color-adesk-attrib` holds a palette index. `material-adesk-attrib` holds a library lookup pair. `f3d.md` §8.2 "An explicit `rgb_color-st-attrib` or" gives the precedence of the two other colour records only. An explicit `rgb_color-st-attrib` or `truecolor-adesk-attrib` on a body or a face gives that target its neutral colour. If neither is present, one appearance binding with a base colour gives the colour.

**Need.** A target can have more than one colour source. We must know the order to select one neutral colour.

## 5. T-splines

### TS-01. `0m cg` wedge partition

**Question.** How does a `0m cg` record's grip run divide into wedges? Two sub-questions remain:

- whether the cross term pairs wedge `k` with wedge `k + 1` or with wedge `k - 1`
- whether the cross term is the product of the two spoke lengths or their minimum

**Known.** `f3d.md` §1.1.1 "A `0m cg vertex wedges S G`" gives the record's fields and fixes the length of `G` at `sum(S[k] + S[k] * S[(k + 1) mod wedges])`. That sum is invariant under reversing the neighbour direction, so the arity rule does not distinguish the two pairings; it only distinguishes the per-wedge block sizes. The two forms of the cross term agree whenever every spoke length is zero or one.

**Need.** A neutral model that keeps grip connectivity must place each grip index in the correct wedge, which the arity rule alone does not fix. A cage with a spoke length of two or more, or one whose per-wedge grip positions can be matched geometrically, settles both sub-questions.

### TS-02. `0m cg` wedge count

**Question.** What fixes a `0m cg` record's wedge count?

**Known.** `f3d.md` §1.1.1 "A `0m cg vertex wedges S G`" defines the count as the number of wedges around the named vertex. It is not the vertex's valence and it is not the number of incident faces: records exist whose vertex has either quantity different from the stored count.

**Need.** Without the rule we cannot write a `0m cg` record for a vertex, only retain one.
