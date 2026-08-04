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

**Known.** `VBL_OFFSURF` has the second spelling `offsetvbsur`. Most of these records include a final solved cache. The cache gives the exact face shape. An offset surface whose support is a `VBL_SURF` stays an `off_spl_sur` with the vertex-blend surface in the support slot: it is accepted on read in that form and is retained in it, and no rewrite into `VBL_OFFSURF` occurs. A `skin_spl_sur`, `skin_spl_sur2`, or `skinsur` payload written from the field order in `f3d.md` §7.3 is refused on read, in a cache-first and in a context-first arrangement. The layouts are speculative, so a refusal separates neither a wrong field order from a wrong field count nor either of those from an unaccepted record name.

**Need.** Some records do not include a cache. For those records, the field layout is the only source of the face shape. We must know the layout to read them and to write them. The decoder keeps these records as opaque bytes now.

### GC-05. Second boolean of the `off_spl_sur` sense pair

**Question.** What does the second boolean of the revision-gated `off_spl_sur` sense pair control?

**Known.** `f3d.md` §7.3 `off_spl_sur` gives the first boolean and the offset construction. The second boolean does not select the offset side and does not move the offset surface. It is set only in records whose first boolean is also set, and those records have a support surface whose stored parameterization is reflected relative to the model. All four states are accepted on read with either sign of the stored distance, and each is retained. A state change alone does not make the reader solve the cache again, so a record whose tail stores a cache gives no surface that separates the states.

**Need.** We must know which value to write when we make this record from a neutral model. `false` is the value for a surface built directly from a support surface and a distance, so the item blocks only the reflected case. **Blocked on a specimen:** a document whose `off_spl_sur` record stores a reflected support and a tail without a cache lets us read the states off the solved surface, and no such document is available to read.

### GC-07. `off_spl_sur` extension run

**Question.** What are the field roles of the extension run in an `off_spl_sur` record?

**Known.** `f3d.md` §7.3 `off_spl_sur` gives the run's field sequence, the prefix states that require it, the zero first integer, and the unit-independent tolerance. A run written to that sequence is accepted on read and is retained field for field, so the sequence is the true layout. The first fields agree with the start of a surface-to-surface intersection payload. That start is a presence logical, a six-integer header, an intersection curve, two pcurve logicals, and a tolerance. The four `-1` integers at the end are more than the three endpoint-term slots of that layout. The five integers after the leading zero are retained verbatim and have no established meaning. A run whose four `-1` integers hold `0 1 2 3` is refused on read, and so is a run whose two post-curve logicals are both true; a refusal does not give the accepted sets.

**Need.** We can now make a record that has a run, but only by copying a run we read. To make one from a neutral model we must know what the five integers, the two logicals, and the four `-1` integers hold, and which values each accepts.

### GC-08. Shared revision-gated surface tail enum values other than `0` and `2`

**Question.** What layouts do the shared revision-gated surface tail enum values other than `0` and `2` select?

**Known.** `f3d.md` §7.3 "**Revision-gated spline-surface forms**" gives the layouts of `0` and `2`. Zero selects the solved cache and its fit tolerance; two replaces both with the two bool-gated parameter intervals and four closure and singularity enums. The decoder keeps a record with any other value as opaque bytes. Values `3` and `4` paired with the complete value-zero cache payload are refused on read. Tails written with the spelling `historical`, `optimal`, `none`, or `summary` and with the payload the same-named `law_spl_sur` mode selects are also refused. Each refusal rejects only the complete submitted value-and-layout pair; it does not reject that numeral with another layout or narrow the accepted spelling set.

**Need.** We cannot read a record with a value other than `0` or `2`. **Blocked on a specimen:** a document whose shared surface tail carries another value gives that value's layout, and a document whose `off_spl_sur` tail carries form `2` shows the cacheless form on that carrier; no such document is available to read.

### GC-13. `cl_loft_spl_sur` tail kind values

**Question.** What are the nonzero values of the revision-gated `cl_loft_spl_sur` tail-kind integer? What layout does each nonzero value select?

**Known.** `f3d.md` §7.3 `cl_loft_spl_sur` gives the kind-zero payload. The revision-gated form takes tail kind `0`; no nonzero kind has a known occurrence in that form. The decoder rejects every nonzero kind. A revision-gated payload written with tail kind `6` or `7` and with the kind-6 or kind-7 payload of the form that is not revision-gated is refused on read. A refusal separates a kind the revision-gated form does not have from a payload the revision-gated form encodes differently, so it does not bound the kind set.

**Need.** We cannot read a record with a nonzero kind.

### GC-16. Token tags of a revision-gated `VBL_SURF` `deg` boundary

**Question.** Which token tags does a revision-gated `VBL_SURF` `deg` boundary use for its location and for its two normals?

**Known.** `f3d.md` §7.3 `VBL_SURF` gives the revision-form tag changes for the type names, the magic location, the support bounds, the curve endpoints, and circle form `3`. It does not give the tags of a `deg` boundary. The decoder writes `0x13` and two `0x14` tokens. In a record whose boundaries are accepted, a `plane` support stores its location in one `0x13` and each of its two directions in one `0x14`, and the leading triple of a boundary is one `0x14`, so the decoder's choice agrees with the tags the same record uses for a location and for a direction. A record whose boundary count is raised by one with a `deg` boundary appended is refused on read, with equal and with distinct normals alike. The appended boundary does not agree geometrically with the other boundaries and the record stores no cache to fall back on, so the refusal does not bear on the tags.

**Need.** A wrong tag makes a file that the reader refuses.

### GC-18. Blend-value selector values

**Question.** We must find three answers:

- whether selector values outside `0` through `7` exist
- the cross-sections selectors `2`, `4`, `5`, and `6` build
- the relation between a selector-seven `edge_offset` length and the solved second-side contact distance

**Known.** `f3d.md` §7.3 `var_blend_spl_sur` gives the layouts of selectors `0` through `7`, the classified cross-section families, the side order, and the contact offsets. Selectors `2`, `4`, `5`, and `6` carry no selector-local payload; each retains its numeric value and continues at the same common tail. Their records store current solved caches, so the caches do not identify the cross-section laws. A rebuilt selector-seven surface meets its first side at the stored offset exactly and its second side short of the stored offset by near one per cent, far above the achieved fit tolerance, so the selector-seven offset is not exactly the second-side contact distance and no other relation is established.

**Need.** A record using a selector outside `0` through `7` would extend the value set. A record whose selector `2`, `4`, `5`, or `6` cache is not current would rebuild into the corresponding cross-section law.

### GC-19. Third-slot raise predicate of a `tvertex` record

**Question.** Which incident edges contribute the third slot's `1e-6` raise?

**Known.** `f3d.md` §6.2 "**Tolerant vertex:**" gives the three slots, the first slot's content — the vertex's own stored tolerance, unit-converted like the other two slots — the second slot's construction from the incident edge-endpoint gaps and tolerant-edge tolerances, the third slot's raise over the incident edges that are not tolerant, and the set-state relation between the second and third slots. The predicate that selects which incident edges contribute the raise is narrower than "every edge that is not tolerant": some records carry a third slot equal to the second where that rule predicts a raise. A record whose second and third slots are equal does not separate candidate predicates.

**Need.** To write the third slot exactly we must know the contribution predicate. The decoder retains the stored slots, so the item blocks only writing from a neutral model.

### GC-21. Revision-gated `loft_spl_sur` type-zero member and ASM-integer gate

**Question.** What do the two nullable spline slots of a type-zero profile member hold, and are they BS2 pcurves or BS3 curves? What form does a type-zero member take in a save-format-23200 stream? At which save format version does the ASM integer start?

**Known.** `f3d.md` §7.3 `loft_spl_sur` gives the two member forms and the save-format gate of the ASM integer. A type-zero member stores two nullable spline slots in place of the support surface and the first flag. The decoder keeps both slots and reads them as BS2 pcurves. Every observed type-zero slot is the null-spline sentinel, and that sentinel is the same token for a BS2 and a BS3 slot, so the slot type is undetermined: only a type-zero member with a non-null slot separates them, by the count of scalars per control point. No type-zero member occurs in a save-format-23200 stream. No stream with a save format version between 22600 and 23200 holds a revision-gated loft. The decoder reads the ASM integer in each stream with a save format version above 22600.

**Need.** We keep the two slots without a change. To write them from a neutral model, we must know what they hold. To write a type-zero member into a save-format-23200 stream, or into a stream with a save format version between 22600 and 23200, we must know if that stream keeps the ASM integer. **Blocked on a specimen:** a document whose type-zero member carries a non-null spline slot separates the slot types by the count of scalars per control point, and a save-format-23200 stream holding a revision-gated loft settles the gate; no such document is available to read.

### GC-23. Cache-first intcurve leading enum values other than `0` and `2`

**Question.** What layouts do the cache-first intcurve leading enum values other than `0` and `2` select?

**Known.** `f3d.md` §7.3 "**Cache-first subtype selection**" gives the layouts of `0` and `2`. The decoder reads both and retains a record with any other value verbatim. Value `2` paired with the complete value-zero cache payload is refused on read; the valid value-two layout replaces that cache rather than retaining it. A `par_int_cur` whose leading enum carries the spelling `summary`, `historical`, or `optimal` over an otherwise untouched cache-first payload is also refused. Each refusal rejects only the complete submitted value-and-layout pair and does not narrow other layouts or spellings.

**Need.** We cannot read a record with a value other than `0` or `2`. **Blocked on a specimen:** a document whose cache-first intcurve leading enum carries a value other than `0` or `2` gives that value's layout, and no such document is available to read. GC-27 gives the separate limit that value `2` reaches.

### GC-24. Binding of the law formula text infix operator `O`

**Question.** What are the precedence and associativity of the infix operator `O` in stored law formula text?

**Known.** `f3d.md` §7.3 "**Law formulas**" gives `O` as composition with the right operand innermost, and gives the `MTRAIL` curve as a rail direction requiring no further construction input. A writer parenthesizes both `O` operands, so stored text never exercises the operator's binding against a neighbouring operator and never chains two occurrences. A `law_int_cur` written from the field order in `f3d.md` §7.3 over a solved curve cache is refused on read, and the refusal covers the smallest form, whose law is `null_law` and which carries no operator at all. The field order and the arity encoding are both speculative, so the refusal bears on neither the operator token spellings nor the binding, and it does not show that the record is unreachable by a different field order.

**Need.** We must know the binding to parse law text that a different writer produced without full parenthesization. Text this codec emits is unaffected, because it parenthesizes both operands.

### GC-25. Payload after a true shared revision-gated surface tail logical

**Question.** What follows the closing logical of the shared revision-gated surface tail when that logical is true?

**Known.** `f3d.md` §7.3 "**Revision-gated spline-surface forms**" ends the tail with six counted float arrays and one logical, for each value of the form enum. The specification gives no payload after that logical, and the decoder ends the tail there for either value.

**Need.** A false logical is the only state the decoder can account for. A carrier whose tail is its last field, such as the revision-gated `cyl_spl_sur`, would end its scope at a true logical and drop the bytes after it without a diagnostic, and would then write the record back short. A carrier with its own fields after the tail, such as `rb_blend_spl_sur` and `var_blend_spl_sur`, reads those fields at the wrong offset instead and keeps the whole record as opaque bytes. No subtype scope has a full-consumption check that would separate the two outcomes from a correct decode. **Blocked on a specimen:** a document whose shared revision-gated surface tail closes with a true logical shows what follows it, and no such document is available to read.

### GC-26. Position of the `sss_blend_spl_sur` third-side graph

**Question.** Does the third-side graph of an `sss_blend_spl_sur` record come between the shared revision-gated surface tail and the three trailing integers, or after those integers?

**Known.** `f3d.md` §7.6 `rb_blend_spl_sur` puts the third-side graph after the tail and before the three `tail_extension` integers. The two-support subtypes end with the tail and those integers, which fixes the integers as the last fields of that scope but does not fix the third-side graph against them. Replacing only the accepted subtype name `rb_blend_spl_sur` with `sss_blend_spl_sur` is refused in a fresh reader session while the unchanged control is accepted. The rename-only payload is therefore not the `sss_blend_spl_sur` grammar, but the refusal does not select either candidate graph position.

**Need.** The decoder and the source-less writer both use the position the specification gives. The wrong position makes every `sss_blend_spl_sur` record fail its decode and stay opaque, and makes a generated record ungrammatical. **Blocked on a specimen:** a document holding an `sss_blend_spl_sur` record fixes the graph position against the trailing integers, and no such document is available to read.

### GC-27. Solved carrier of a cache-first intcurve that stores no cache

**Question.** Which curve gives the parameter domain of a cache-first intcurve record whose leading enum is `2` and whose construction stores no curve block?

**Known.** `f3d.md` §7.3 "**Cache-first subtype selection**" gives the layout of leading enum `2`: the record stores a bool-gated curve interval and a closed-form enum in place of the solved-curve cache and the fit tolerance that enum `0` stores. The shared cache-first context takes its parameter domain from the record's solved curve, and the record-level search takes the first curve block in the record as that curve. A form-`2` record therefore has a solved carrier only when a nested construction stores a curve block.

**Need.** A form-`2` record that stores no curve block anywhere gives the context no parameter domain, so the decoder retains the record verbatim and the neutral model loses the curve. To read such a record the shared context and every carrier that builds it must accept a record with no solved curve. Whether the record then takes its domain from the interval the form stores, from the support surfaces, or from a curve outside the record is not established. **Blocked on a specimen:** a document holding a form-`2` record whose construction stores no curve block settles which of the three the domain comes from, and no such document is available to read.

### GC-28. Parameter chart of a procedural spline support cache

**Question.** How does a pcurve in a procedural spline support's construction chart map to the parameter chart of that support's solved NURBS cache?

**Known.** A cache-first intcurve support can store `spline`, a subtype-table reference to a procedural spline-surface construction, and four optional bounds. The referenced construction supplies a solved NURBS cache. The intcurve pcurve uses the procedural construction's parameter chart. That chart is not necessarily the solved cache's chart. A `cl_loft_spl_sur` support can map one construction-chart isoline to a nonlinear curve in the solved cache chart. The intcurve and the cache therefore do not establish a direct pcurve-on-surface relation without a chart map.

**Need.** The decoder currently attaches the construction-chart pcurve directly to the solved NURBS support. This relation is invalid when the charts differ. We must retain or derive the exact chart map before the neutral support relation can be complete. A fitted map is not sufficient because it does not preserve the stored construction semantics.

## 2. Container, header, and design records

### DR-03. ACT table trailing GUID run

**Question.** What does the run of LP-UTF16 GUIDs after an ACT table's counted entry list hold? What gives its length?

**Known.** `f3d.md` §8.1 "**The ACT segment.**" gives the two-byte prologue, the counted `(reference, entity key)` entries, and the join from each entry to its change group. The run after the last entry is a sequence of 36-character GUID strings and has no count of its own.

**Need.** We must know the run to write an ACT table. A reader that stops after the counted entries keeps every change-version join, so the item blocks writing only.

### DR-05. Recipe records of a non-locus parameter companion

**Question.** How do the recipe records inside one non-locus indexed-parameter-companion variant relate to each other as an operation?

**Known.** `f3d.md` §8.1 "Within a dimensional companion," gives the containment order and the retention order. `f3d.md` §8.1 "An edge recipe's words" gives the edge-recipe-subsequence join. `f3d.md` §8.1 "A recipe-backed linear dimension" gives the measurement rule for a recipe-backed linear dimension that has no locus.

**Need.** We must know the operation to build a neutral dimension from more than one recipe record.

### DR-09. Sheet-metal `EdgeFlange` to-object height extent

**Question.** What is the layout of an `EdgeFlange` frame whose height extent terminates at a selected object rather than at a distance?

**Known.** `f3d.md` §8.1 "A single-edge `EdgeFlange` scope has" gives the distance-extent layout, the header shift, the bend-position values, the height-datum values, and the edge-width mode. A to-object frame adds three ordered references and inserts a marked reference pair into the fixed operation section between the height-owner reference and the result-record run. Its frame length does not satisfy the distance-extent length relation for any result-record count, so the to-object form is a separate layout and the decoder refuses it. The height-datum discriminator still carries the outer-faces value in a to-object frame, where the height datum has no effect.

**Need.** A to-object height extent has no neutral extent without the inserted pair's roles. The decoder retains the scope as a native record, so the item blocks the extent semantics of that form only.

### DR-09A. Sheet-metal `Hem` discriminators and form layouts

**Question.** We must find three answers:

- the offset of the `Hem` bend-position discriminator
- the meanings of the `Hem` direction discriminator and the direction-reversal byte
- the layout each hem form selects

**Known.** `f3d.md` §8.1 "A `Hem` scope has" gives the offsets of the flat form. The header shift defined for `EdgeFlange` applies to `Hem` as well. The u32 at offset `121 + S` is `4` in a frame whose authored bend position is adjacent, so that offset is not the bend-position discriminator; the `EdgeFlange` bend-position values do not apply to it. The direction-reversal byte at offset `119 + S` is clear both in a frame authored with the default direction and in one authored with the direction reversed, so it does not carry that state either.

The hem-form discriminator at offset `85 + S` is `3` for the flat form. The open, rolled, and teardrop forms each replace the gap and length parameter owners with the owners their form needs, so their ordered reference tables differ in content and the teardrop form adds a ninth reference. The decoder reads the flat form only.

**Need.** A hem has no neutral operation without the form layouts and the direction meaning. **Blocked in part on a specimen:** the closed, rope, and double forms are not available to read.

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

### DR-14. `SurfacePatch` boundary-side and scale values

**Question.** Which field holds the boundary side of a `SurfacePatch` component? What does `PatchScale` hold?

**Known.** `f3d.md` §8.1 "A surface-patch boundary-settings record has" gives the member order, the offsets, and the types. `PatchContinuity` value `0` imposes positional continuity, `1` imposes tangency, and `2` imposes curvature; the value is stored per boundary component, and one patch can impose a different condition on each of its boundaries.

`PatchFlip` does not hold the boundary side. It carries the value `2` in every record, including in patches that differ only in the authored boundary side. `PatchScale` is `-1.0` in every record, so no mapping from it to a neutral value is decidable in either direction. `IsSeedSel` is set on exactly one boundary component of a patch.

**Need.** A neutral patch needs the boundary side to place its generated surface, and the neutral operation carries one continuity, so a patch imposing more than one condition needs a per-boundary neutral carrier before its continuity transfers completely. A tangency or curvature weight authored away from its default separates `PatchScale` from a constant.

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

**Known.** `f3d.md` §8.1 "A nested entity-selection member" states that an identity absent from the preceding state gives no candidate. `f3d.md` §8.1 "An Extrude selection resolves" gives a fallback chain that ends in native retention. `f3d.md` §8.1 "The first identity-wrapper record" gives the presence encoding of the optional slot. The marker is zero when the slot is absent and one when the slot is present.

**Need.** Each unknown makes one Extrude selection fall back to native retention. The neutral model then has no selection.

### DR-18. Extrude extent arbitration

**Question.** Which field determines the extent form when an extent discriminator and the stored termination reference disagree?

**Known.** `f3d.md` §8.1 "The extent form is carried by" gives the two per-side discriminators, their enum, and the parameter and reference set each value implies. Every implication holds in both directions, so no record separates the discriminator from the reference: the two never disagree. Extent value `3` does not occur, so neither its termination-entity search nor the tool-body extension mode of value `4` is exercised.

**Need.** A writer needs to know which field a reader follows before it can emit a record where the two differ, and whether value `3` may be written without a termination reference. A design authored with a to-object termination whose side is then switched to `to next` without clearing the object settles it.

### DR-19. Construction-group fields

**Question.** What do the construction-group scalar fields hold? What does the variant byte control? What do the group-role values outside the defined feature-specific sets mean?

**Known.** `f3d.md` §8.1 "Every `Extrude`, `Extrusion`, `Fillet`," gives the member order and the value limits. The group holds a nonzero u32 `ordinal`, a nonnegative finite f64 `scalar`, and a second copy of `ordinal` that one container generation omits. The value of `variant` is zero or one. The same paragraph defines the Extrude roles `0x08`, `0x41`, and `0x11` only. Roles `0x81` and `0x100` name no defined operand family, and `0x100` does not fit in one byte. `scalar` is not equal to a compact-parameter value in the same feature scope, with or without unit scaling. `ordinal` is below 256, has one value for all groups of one feature scope, and does not decrease with the record index. A `variant` value of one occurs only on a scope that has no history state. The two optional references that follow the member run, and the count that opens the identity run, have no reader.

**Note.** The u32 word before `role` is zero in every record, so a reader that takes `role` as a u64 starting at that word and a reader that takes it as a u32 starting after it name the same value. The decoder takes the u64. Nothing separates the two readings.

**Need.** We must know the field meanings to write a construction group from a neutral model. The role value `0x0000000500000000` in an Extrude scope is one case of an undefined role.

### DR-19A. Entity-tracking path discriminators

**Question.** What do the signed selector and kind fields of a construction-operand entity-tracking path select?

**Known.** `f3d.md` §8.1 "A construction-operand entity-tracking path" gives the complete wrapper and carrier grammar. The selector is a signed i32, the kind is a u32, and the two optional related identities retain their ordered positions independently. The primary and related identities also occur as persistent Sketch-curve identities. Selector values `-1`, `1`, and `2` and kind values `1`, `2`, and `3` occur.

**Need.** The decoder retains both discriminators and every identity. Their semantic meanings are required to generate an entity-tracking path from neutral selection intent.

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

- the thirty bytes of the class tail
- the flag byte after the horizontal-alignment enum and the flag byte after the vertical-alignment enum
- the five bytes between a `txt_tag` record's rotation and its colour components, the two bytes between its height and its anchor coordinates, and the eleven bytes between those coordinates and its text string, ten of them below class version 4
- the targets of the reference run a `txt_tag` record writes after its text string, the three bytes and eight unclassified bytes around its font weight, and the pairs of its leading block

**Known.** `f3d.md` §8.1 "Sketch text occupies two record classes" gives the two class GUIDs and the identity keys. `f3d.md` §8.1 "In a `textex_tag` record the property block" and `f3d.md` §8.1 "In a `txt_tag` record the twenty-nine bytes" give each class's members up to the text string, including the anchor-point coordinates of the `txt_tag` class. `f3d.md` §8.1 "A `textex_tag` record writes two optional" and `f3d.md` §8.1 "A `textex_tag` record's class tail opens" give the remaining members and the placement transform. A `txt_tag` record's f64 directly after the property block is its stored rotation in radians about the anchor; zero is explicit. The `textex_tag` form derives frame-text rotation and anchor from its transform. The colour components come from the run ahead of the font family. The decoder reads both forms. A `txt_tag` record stores no width factor.

The thirty-byte class tail is `u32 0`, `u8 1`, five f32 `(0, 0, 0, 1, 1)`, `u32 0`, and `u8 0`. It is the same in every record of both classes. Its field boundaries are thus fixed, but no field in it changes, so no field in it has a meaning we can read. Of the three bytes after the horizontal-alignment enum, only the first is ever set. The single byte after the vertical-alignment enum is set when that byte is set, and clear when it is clear. The two are one flag written twice, or one flag and an echo of it. The alignment enums change independently of each other, so neither flag byte continues an alignment value. `f3d.md` §8.1 "Sketch text occupies two record classes" gives the leading block that both classes carry.

**Need.** We must know the meanings to write sketch text from a neutral model. A record that sets one of the two flag bytes and clears the other separates them. A class tail that differs from the constant above gives its fields a meaning. Nothing else in a sketch-text record changes with either.

### DR-27. Sketch-relation class-member meanings

**Question.** What do these members of a sketch-relation subclass hold?

- the three `u8` flags of the tangency class and the three of the rectangular-pattern class
- the u32-counted reference run of the rectangular-pattern class
- the u64 key and the u64 values of the pattern-table map, and the u32 of the pattern-table run
- the first of the two text-frame references
- the zero `u8` that closes the circular-pattern class

**Known.** `f3d.md` §8.1 "A sketch-relation class writes" gives the member sequence of each class, and each sequence closes the record on its exact end. The fields above have a width and no meaning. The decoder consumes them and transfers nothing from them.

**Need.** A pattern relation must be written back from a neutral model, and these members must carry the values the source would have written. The map keys and values are small integers in the range of record indices, so they may be a per-instance record grouping that a writer must rebuild rather than copy.

### DR-28. `VisibilityAttribute` values on display-scene sketch-curve nodes

**Question.** What design state does the `VisibilityAttribute` value of a display-scene sketch-curve node give?

**Known.** `f3d.md` §1.1 gives the `OGS.BlobFolder` family's role. The scene graph attaches attributes to each drawable node. An attribute is the length-prefixed attribute name, a five-byte prologue, and the attribute payload. The `VisibilityAttribute` payload is one byte with the value `0` or `1`. The attribute occurs on sketch-domain nodes and on work-geometry nodes; body, face, and edge nodes do not carry it. The value is `0` on every sketch-constraint, sketch-dimension, sketch-point, sketch, work-plane, work-axis, component, and group node. On a sketch-curve node the value is `0` or `1`. The value does not follow the curve type, the effect colour, the line style, or the owning sketch: one sketch holds curves with both values.

**Need.** The scene graph names the design entity of each node, so a sketch-curve node joins to a design curve record. If the value gives a curve property, that property has no other carrier and the scene graph is not a pure display cache. If the value gives a render-state decision, a decoder can drop the whole family. **Blocked on a specimen:** a document that hides one body or one sketch separates the two answers, and no such document is available to read.

**Note.** The attribute name does not give the meaning. Both direct readings of the value are inconsistent with the values on the other node kinds.

### DR-29. `EntityGenesis` values `0x4` and `0x8`

**Question.** What do the `EntityGenesis` values `0x4` and `0x8` mean?

**Known.** `f3d.md` §8.1 "A sketch-curve record contains" gives the origin bitfield and assigns bits `0x2`, `0x80`, and `0x100`. The values in the records available are `0x0`, `0x2`, `0x4`, and `0x8`. A document's sketch curves can carry `0x8` while some of their centre points carry `0x0`, and a sketch-text record carries `0x4` while its frame curves carry `0x0`, so neither value follows the record class or the owning sketch.

**Need.** A writer must emit the value the source would have written. Without the bit meanings, a neutral model cannot choose between `0x0`, `0x4`, and `0x8`.

### DR-30. `sketch_attrib_def` member `ref_b`

**Question.** What does `ref_b`, the second member of the `sketch_attrib_def` payload, name?

**Known.** `f3d.md` §6.6 "`sketch_attrib_def` is source-link metadata" names it, gives its position in all three payload forms, and gives its range. It is zero in most links, and the decoder keeps a non-zero value as written. A non-zero value repeats across the links of one stream and is drawn from a small set per document. Where a document has sketch text, the values that recur most are the Design entity ids of its sketch-text records, and the `sketch_curve_id` beside them matches no sketch-curve record's persistent identity in that document. Where a document has no sketch text, non-zero values still occur and the `sketch_curve_id` beside them does match a stored sketch-curve identity. So the field names neither a sketch text in particular nor, by itself, the namespace `sketch_curve_id` is drawn from. One document spells it `18446744073709551615` throughout, the all-ones 64-bit pattern, where `0` is the value every other document writes for a link with no such reference.

**Need.** A writer must choose the value to emit for a link a neutral model does not carry. Without the meaning, only a link decoded from source restores it.

### DR-31. Other `Pipe` section and hollow forms

**Question.** Which primary-header values select square, triangular, and hollow `Pipe` sections? How is section size measured for the noncircular shapes, and where is hollow-wall placement encoded?

**Known.** Primary-header offset 29 value `1` selects a circular section and offset 30 value `1` selects a filled section. For that form, scalar ordinal two is the outside diameter and scalar ordinal three is an inactive positive thickness. The section is one filled disk and has no inner boundary. The settings reference contains a u32 and one finite double.

**Need.** A writer needs the selector values and dimension conventions for every supported generated section. Hollow forms also need the direction in which thickness changes the section boundary. **Blocked on specimens:** settling these forms needs otherwise equal pipes with each section shape and with the hollow option both off and on.

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

**Known.** `f3d.md` §1.4 "**Placement.**" gives the class, the layout, the instance discriminator, and the identity-marked matrix. The counted run reaches both local and cross-document targets. The two closing references name the same pair of entities for every placement of one document, so neither depends on the placement. A placement record can also carry the UTF-16 string `GatedByParent`; its position in the member sequence, its gate, and its value are not established.

**Need.** We must know the targets to write a complete occurrence placement. A reader takes the target path, the discriminators, and the transform without them.

## 4. Material assets

### MA-03. Distance unit-tag values

**Question.** What are the unit tags of a Distance value other than the three known length tags?

**Known.** `f3d.md` §8.2 "Boolean stores one u8." gives the tag structure `(quantity class << 12) | unit index`, with the unit index one-based, and gives three length tags: `0x200d` is centimetre, `0x200e` is millimetre, and `0x2016` is inch. The decoder converts these three to millimetres and returns no unit for every other tag. The tag is a property of the asset and does not track the document display length unit, so a change of that unit does not enumerate further tags. The schema `unit` attribute of a Distance property does not predict the tag, and one record mixes tags across its own members, so the attribute does not bound the tag set.

**Need.** A Distance with an unknown tag gets no unit. The neutral model then has a value with no scale. We must know which further unit indexes the length class `0x2` has, and whether a Distance takes a quantity class other than length.

### MA-04. Texture map-channel values

**Question.** What does each texture map-channel integer value mean?

**Known.** `f3d.md` §8.2 "`UnifiedBitmapSchema` and `BumpMapSchema` records" names the map-channel property. The application defines the value meanings.

**Need.** We must know the meanings to map a texture to the correct channel in a neutral model.

### MA-08. Omitted texture map-channel member

**Question.** Which member does a `TextureMap2dSchema` closure omit from its value block?

**Known.** `f3d.md` §8.2 "An `InstanceProperties` record opens with" states that such a block is one four-byte slot shorter than its closure, and that only omitting `texture_MapChannel_ID_Advanced` or `texture_MapChannel` leaves every surviving member at its schema default. The two are byte-degenerate: both are four-byte integers at adjacent positions, and the surviving pair reads `1` and `0` under either choice, which are the two members' declared defaults in either assignment.

**Need.** A writer must emit the same member set the reader expects, and the two choices shift every following member by four bytes. A texture asset authored with a map channel other than `1`, or with a non-default advanced channel id, separates them.

### MA-05. Canvas visibility, mirroring, and crop

**Question.** Which Canvas fields hold the visibility state, the mirroring state, and the crop state?

**Known.** `f3d.md` §8.1 "A `Canvas` scope names" gives the Canvas geometry record. It holds the opacity, the plane frame, both boundary segments, the label, and the image asset. It names no visibility field, no mirroring field, and no crop field. The decoder reads the opacity and the plane frame only.

**Need.** A neutral canvas needs these three states to show the image correctly.

### MA-07. Precedence of library colour records

**Question.** What is the precedence of the `color-adesk-attrib` record and the `material-adesk-attrib` record against direct colours and appearance assignments? What do the twelve bytes and the eight bytes of a per-face assignment entry hold?

**Known.** `f3d.md` §8.2 "Color attribute records include" gives the content of both records. `color-adesk-attrib` holds a palette index. `material-adesk-attrib` holds a library lookup pair. `f3d.md` §8.2 "An explicit `rgb_color-st-attrib` or" gives the precedence of the two other colour records only. An explicit `rgb_color-st-attrib` or `truecolor-adesk-attrib` on a body or a face gives that target its neutral colour. If neither is present, one appearance binding with a base colour gives the colour. `f3d.md` §8.2 "Per-face appearance assignments live" gives the assignment entry; its two unnamed byte runs have a width and no meaning.

**Need.** A target can have more than one colour source. We must know the order to select one neutral colour, and the entry byte runs to write a per-face assignment from a neutral model.

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

### TS-03. `e`-record scalar

**Question.** What geometric or topological quantity does the scalar after an `e` record's half-edge root hold?

**Known.** `f3d.md` §1.1.1 "Topology uses zero-based indices in record order." gives the field position and its finite-value invariant. The scalar is present on smooth and creased edges. It is usually near one, but it also takes exact binary fractions and other positive values. Crease membership is stored independently by the `ec` records, so the scalar is not the edge's crease flag. The decoder retains the complete `.tsm` entry but does not transfer this scalar to the neutral subdivision cage.

**Need.** We must know the quantity and its endpoint convention before the decoder can assign it to `SubdEdge.sharpness`, `sector_coefficients`, or a new neutral field. Assigning it to sharpness without that distinction would mark smooth edges as sharp.

### TS-04. `105plane` coefficient model

**Question.** What geometric values do the twelve f64 operands of a `105plane` record encode, and which operands use the cage coordinate scale?

**Known.** `f3d.md` §1.1.1 "A `105sym 0` record" gives the record arity and its relationship to the six symmetry correspondence maps. The maps identify the complete face, edge, and vertex involution without using the plane coefficients. Every coefficient is finite.

**Need.** We must identify the coefficient grouping and coordinate scale before projecting the symmetry plane into a neutral geometric plane or writing a new symmetry block from neutral data.

## 6. Mesh geometry

### PM-01. `.paramesh` undecoded streams and container fields

**Question.** We must find six answers:

- how an `r0` stream frames its elements, because its descriptor sets `U`
- what `r0` holds
- what `r0i` and `r1` hold
- what the `r2` per-triangle value selects
- what descriptor `T` values other than `0`, `1`, and `3` select, and what `U` selects
- what the protobuf message fields other than the stream registry, the resource GUIDs, and `fusion_uuid` hold

**Known.** `f3d.md` §1.1.2 "The container layout is" gives the container framing, both compressed and raw stream encodings, and the descriptor value types. The `v` and `t` streams are decoded. The `t` stream's implicit initial corner is the unique start that keeps every reconstructed corner inside the vertex domain; every stored difference except the terminal value contributes one corner. `r2` carries one zero u32 per triangle while imported per-triangle colours instead add the `r3`, `r3i`, and `r4` family, so `r2` is not that colour selector.

An `r0` stream declares three components of type f32 and boolean `U = true`. Where the mesh is a cube, every component in it is `-1`, `0`, or `1`, which is the component set of the six face normals of a cube, and the stream holds sixty-four components, which three does not divide. Where a mesh has eight vertices, twelve triangles, and thirty-six corners, `r0i` and `r1` each hold twenty-four values, so neither stream holds one value per vertex, per triangle, or per corner. The `r0i` values accumulate to an increasing sequence that ends below the component count of `r0`, so `r0i` can hold offsets into `r0`. The `r1` values accumulate to a sequence that goes below zero, so `r1` does not hold offsets. The colour-family streams have established framing but their `r3i` to `r3` indexing and `r4` field semantics are not decoded.

**Need.** The decoder keeps the auxiliary streams as opaque bytes. We must know their contents, the remaining descriptor selectors, the colour-family indexing, and the message fields to write a container from a neutral model.

### PM-02. Mesh Design-record classes without decoded content

**Question.** We must find two answers:

- what these five mesh-joined record classes hold:
  - `443807AD-8025-41A3-8A50-5157579C3D78` (add-in `ParaMesh`)
  - `6FC173DC-C7E3-402C-A8C0-891A26DADF8D` (add-in `ParaMesh`)
  - `E5B3F49A-D8D0-4EEF-BC2B-FCDDAEF9745E` (add-in `ParaMesh`)
  - `99F6967E-ED35-4222-B906-5CCF0AC70B53` (add-in `Fusion`)
  - `f85f2e62-7627-4922-a16d-53e1275d2aac` (add-in `Scene`)
- which of the two matrices of an `EA90DA22-556C-4C61-89BB-20C2681B7A9D` record governs the map from container coordinates to model space, and whether that matrix is the complete map

**Known.** `f3d.md` §8.1 "A mesh body's geometry container" gives the three decoded classes. Each of the five classes above occurs once per mesh body and does not occur in a document without one. The `EA90DA22-556C-4C61-89BB-20C2681B7A9D` record stores two equal affine matrices. Applying either matrix to container coordinates in model centimetres supplies the complete nonuniform scale and translation of a placed mesh. Separate mesh bodies retain separate containers, identities, and matrices even when their geometry bytes are equal.

**Need.** We must know the five payloads to write a mesh body from a neutral model, which duplicate matrix governs if they differ, and how a negative-determinant matrix affects triangle winding.
