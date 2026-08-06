# Autodesk Fusion 360 `.f3d`: Open Items

This document lists the parts of the F3D format that we do not know. The specification `f3d.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

A reference to the specification gives the section number and the start of the paragraph. An example is `f3d.md` §3.1 "**The ACT segment.**". Do not use line numbers. Line numbers become incorrect when the specification changes. The `scripts/check-doc-anchors.py` command makes sure that each reference finds exactly one paragraph.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Container, header, and design records

### DR-03. ACT table trailing GUID run

**Question.** What does the run of LP-UTF16 GUIDs after an ACT table's counted entry list hold? What gives its length?

**Known.** `f3d.md` §3.1 "**The ACT segment.**" gives the two-byte prologue, the counted `(reference, entity key)` entries, and the join from each entry to its change group. The run after the last entry is a sequence of 36-character GUID strings and has no count of its own.

**Need.** We must know the run to write an ACT table. A reader that stops after the counted entries keeps every change-version join, so the item blocks writing only.

### DR-05. Recipe records of a non-locus parameter companion

**Question.** How do the recipe records inside one non-locus indexed-parameter-companion variant relate to each other as an operation?

**Known.** `f3d.md` §3.1 "Within a dimensional companion," gives the containment order and the retention order. `f3d.md` §3.1 "An edge recipe's words" gives the edge-recipe-subsequence join. `f3d.md` §3.1 "A recipe-backed linear dimension" gives the measurement rule for a recipe-backed linear dimension that has no locus.

**Need.** We must know the operation to build a neutral dimension from more than one recipe record.

### DR-09A. Sheet-metal `Hem` form selector and direction

**Question.** Which field selects the hem form, which field carries the hem direction, and what is the layout of the rolled and teardrop frames?

**Known.** `f3d.md` §3.1 "A `Hem` scope names one parameter" gives the two-owner layout, the header shift, the parameter source kinds of each form, and the four retained fields.

The parameter set separates a rolled hem, which owns `HemRadius` and `HemAngle`, and a teardrop hem, which owns three parameters, from the two-owner forms. It does not separate a flat hem from an open one: both own `HemGap` and `HemLength`. A flat hem's `HemGap` holds a small value its form does not use, which is a value difference and not a selector.

The four retained fields each hold one value across the flat, open, rolled, and teardrop forms and across both authored direction states, so none of them carries the form or the direction. The retained u32 at offset `121 + S` is not the bend position either: it holds `4` in hems whose authored bend position is adjacent, which `EdgeFlange` shows is code `3`. The gap-and-length, radius-and-angle, and gap-length-radius owner layouts have distinct fixed-section lengths and rule-radius offsets.

A rolled frame places its two owner references 13 bytes apart rather than 11, and a teardrop frame adds a third owner reference and moves the group references by ten bytes. Neither form uses the gap-and-length owner layout. The parameter source kinds identify the rolled and teardrop input sets, but the fixed fields do not identify flat versus open or either direction state.

**Need.** A hem has no neutral operation without the form selector and the direction carrier. The fixed owner layouts settle the rolled and teardrop input sets, but the flat/open distinction and direction remain unresolved.

### DR-12. Placement `refType` values

**Question.** What construction does each `refType` value of the placement class select?

**Known.** `f3d.md` §3.1 "A `Sketch` scope joined" gives the member order, the version gates, and the identity-marked matrix. `refType` is a u32 whose value does not change the member sequence, and the reference members it selects between — `rOrigin`, `rXAxis`, `rZAxis`, `rEdgeReferences`, `rPlaneReferences`, and `values` — are present at every value.

**Need.** We must know which references a given `refType` requires to write a placement that Fusion re-solves rather than accepts as stored.

### DR-13. `WorkPoint` `refType` values

**Question.** What construction does each `refType` value of the point-data class select?

**Known.** `f3d.md` §3.1 "A direct `WorkPoint` scope" gives the member order, the version gates, and the counted input-reference run. The stored `point3d` is the solved position for every value, so a reader needs no join to place the point. The decoder retains the `refType`, the serialized input count, and every input reference without assigning a rule-specific meaning.

**Need.** A writer must emit the `refType` that matches the inputs it writes, and a neutral model that edits an input must know which rule re-solves the point.

### DR-14. `SurfacePatch` boundary-side and scale values

**Question.** Which field holds the boundary side of a `SurfacePatch` component? What does `PatchScale` hold?

**Known.** `f3d.md` §3.1 "A surface-patch boundary-settings record has" gives the member order, the offsets, and the types. `PatchContinuity` value `0` imposes positional continuity, `1` imposes tangency, and `2` imposes curvature; the value is stored per boundary component, and one patch can impose a different condition on each of its boundaries.

`PatchFlip` does not hold the boundary side. It carries the value `2` in every record, including in patches that differ only in the authored boundary side. `PatchScale` is `-1.0` in every record, so no mapping from it to a neutral value is decidable in either direction. `IsSeedSel` is set on exactly one boundary component of a patch.

**Need.** A neutral patch still needs the boundary side to place its generated surface. A tangency or curvature weight authored away from its default separates `PatchScale` from a constant.

### DR-15. Recipe fields for ambiguous edge operands

**Question.** Which recipe field assigns the active B-rep edge identity when the candidate set is empty, disjoint, or has more than one intersection?

**Known.** `f3d.md` §3.1 "An edge operand has" gives the result of these cases. An empty leading reference set identifies an unresolved edge-bearing operand. A disjoint set does the same. `f3d.md` §3.1 "For each selector, an" keeps an unresolved identity without a change.

**Need.** The neutral model needs one edge identity per operand.

### DR-16. Extrude face-recipe candidate discriminator

**Question.** Which field selects one active B-rep face candidate when two rules both fail?

**Known.** `f3d.md` §3.1 "An `Extrude` face-group member" gives the two rules that work. The first rule applies when every effective active candidate has a support mapping and every mapping names the same non-empty predecessor-face set. The second rule uses a counted-boundary predecessor set.

**Need.** The neutral model needs one face per recipe member.

### DR-17. Extrude selection unknowns

**Question.** We must find five answers:

- what an identity that is absent from history denotes
- which field separates two profile loops that meet at the same ordered persistent Sketch points
- which field selects one of several closed spatial-Sketch profiles
- what the context UUID names
- what the optional slot of the fixed member tail holds

**Known.** `f3d.md` §3.1 "A nested entity-selection member" states that an identity absent from the preceding state gives no candidate. `f3d.md` §3.1 "An Extrude selection resolves" gives a fallback chain that ends in native retention. `f3d.md` §3.1 "The first identity-wrapper record" gives the presence encoding of the optional slot. The marker is zero when the slot is absent and one when the slot is present.

**Need.** Each unknown makes one Extrude selection fall back to native retention. The neutral model then has no selection.

### DR-18. Extrude extent arbitration

**Question.** Which field determines the extent form when an extent discriminator and the stored termination reference disagree?

**Known.** `f3d.md` §3.1 "The extent form is carried by" gives the two per-side discriminators, their enum, and the parameter and reference set each value implies. Every implication holds in both directions, so no record separates the discriminator from the reference: the two never disagree. Extent value `3` does not occur, so neither its termination-entity search nor the tool-body extension mode of value `4` is exercised.

**Need.** A writer needs to know which field a reader follows before it can emit a record where the two differ, and whether value `3` may be written without a termination reference. A design authored with a to-object termination whose side is then switched to `to next` without clearing the object settles it.

### DR-19. Construction-group fields

**Question.** What do the construction-group scalar fields hold? What does the variant byte control? What do the group-role values outside the defined feature-specific sets mean? What does the boolean of the compact flag record a trailing reference names select?

**Known.** `f3d.md` §3.1 "Every `Extrude`, `Extrusion`, `Fillet`," gives the member order and the value limits. The group holds a nonzero u32 `ordinal`, a nonnegative finite f64 `scalar`, and a second copy of `ordinal` that one container generation omits. The value of `variant` is zero or one. The same paragraph defines the Extrude roles `0x08`, `0x41`, and `0x11` and defines Fillet role `0x04` as the full-round center-face form. In that form the group has one compact persistent-identity member, one trailing compact flag whose boolean is `true`, and `variant = 0`; the member's bounded-face operand supplies the center face and the flag requests automatic side-face inference. Roles `0x81` and `0x100` name no defined operand family, and `0x100` does not fit in one byte. `scalar` is not equal to a compact-parameter value in the same feature scope, with or without unit scaling. `ordinal` is below 256, has one value for all groups of one feature scope, and does not decrease with the record index. The two optional references that follow the member run, and the count that opens the identity run, have no reader. The compact flag record has the automatic-side meaning in the Fillet full-round form; its meaning in other group families remains unsettled.

**Note.** The u32 word before `role` is zero in every record, so a reader that takes `role` as a u64 starting at that word and a reader that takes it as a u32 starting after it name the same value. The decoder takes the u64. Nothing separates the two readings.

**Need.** We must know the scalar, variant, optional-reference, and compact-flag meanings for the remaining construction-group forms before writing them from a neutral model. The role value `0x0000000500000000` in an Extrude scope is one case of an undefined role.

### DR-19A. Entity-tracking path discriminators

**Question.** What do the signed selector and kind fields of a construction-operand entity-tracking path select?

**Known.** `f3d.md` §3.1 "A construction-operand entity-tracking path" gives the complete wrapper and carrier grammar. The selector is a signed i32, the kind is a u32, and the two optional related identities retain their ordered positions independently. The primary and related identities also occur as persistent Sketch-curve identities. Selector values `-1`, `1`, and `2` and kind values `1`, `2`, and `3` occur.

**Need.** The decoder retains both discriminators and every identity. Their semantic meanings are required to generate an entity-tracking path from neutral selection intent.

### DR-20. Face-recipe node scalar fields

**Question.** What is the topology meaning of the root scalar, the prelude scalars, and the side-clause scalars of a face-recipe node?

**Known.** `f3d.md` §3.1 "An `Extrude` face-group member" gives the node structure. The payload is a `-1`-delimited root scalar, two one-word prelude runs, and two topology side clauses. The root scalar, the first prelude scalar, and every side scalar keep their source field order and their repetitions. `f3d.md` §3.1 "The face-node `-1`-delimited grammar" states that the delimiter value does not change the retained field value or the side structure. `f3d.md` §3.1 "The structured edge-recipe program" gives a role to these scalars in an **edge** recipe only.

**Need.** We must know the meaning to build face topology from a recipe without the edge-recipe rule.

### DR-21. Body identity of a `Move` or `RemoveBody` group

**Question.** Which join connects a `Move` or `RemoveBody` construction-group identity with role `0x0000000400000000` to a neutral body identity?

**Known.** `f3d.md` §3.1 "A `Move` scope references" and `f3d.md` §3.1 "A `RemoveBody` scope references" give the record layout.

**Need.** Without the join, the neutral model has no body selection for these two features.

### DR-22. Form-33 `Combine` body recipe

**Question.** Which field selects the input body for a form-33 `Combine` body-recipe identity that intersects more than one input body and has no matching occurrence elsewhere in the Design stream?

**Known.** `f3d.md` §3.1 "A `Combine` scope stores" gives the complete persistent identity and the agreement rule. The identity is the stream GUID, the asset GUID, the context GUID, the ordered `(Design reference, form)` clauses, the recipe Design id, and the recipe selector. Repeated occurrences of that identity select the same stable history body when every resolved occurrence agrees.

**Need.** This item is the case in which the agreement rule has no input. The neutral model then has no body selection.

### DR-23. `Draft` outward convention

**Question.** Which stored carrier fixes the outward-material convention of a `Draft` scope?

**Known.** `f3d.md` §3.1 "A `Draft` scope has" gives the field roles and both group forms. The first scalar is a finite signed draft angle in radians, including zero, and the second reserves the opposite-side angle at zero. A neutral-plane draft has one role-`0x0000002100000000` face-recipe group. A parting-line draft has two such groups: one single-member entity-selection group names the WorkPlane at primary identity plus one, and the other carries the parting-tool face recipes. The WorkPlane's third matrix column supplies the pull direction and its feature is the pull-plane dependency.

The signed angle and the WorkPlane pull direction are independent fields. The outward-material convention has no identified carrier, so the neutral model leaves `outward` unset.

**Need.** Identify the stored carrier for the outward-material convention without deriving it from the angle sign or pull direction.

### DR-24. Class-365 whole-body operand fields

**Question.** What do the class-365 whole-body operand fields after the asset UUID and the context UUID hold? This question excludes the bounded nested-record join and the body-recipe join.

**Known.** `f3d.md` §3.1 "A class-365 whole-body member" gives the reference count, the ordered `(Design reference, form)` pairs, the asset UUID, the context UUID, the `u32 2`, the four zero bytes, the paired header, and the nested indexed headers. The `u32 2` is a literal that never varies. The four bytes after it are not always zero, and the u32 after those takes a broad range of small values. The `0x01`-tagged value before the two UUIDs is a reference whose target is the containing entity plus three, so the two zero bytes after it are that reference's flags.

**Need.** We must know what the two variable u32 select to write a complete class-365 member.

### DR-25. Base Feature six-byte fields

**Question.** What do the six-byte fields after the Base Feature body suffixes and after the Base Feature record references hold?

**Known.** `f3d.md` §3.1 "**References.**" resolves the six bytes that follow an eleven-byte element: they are the target entity ID's high half and the two reference flags. A fifteen-byte element already carries the full entity ID, so its six bytes are instead the two flags and a further u32 member of the element. The `u16 0` then `u32 1` form is that member holding `1`: the flags are both clear, and a cross-segment reference would put `1` in the second flag byte rather than in the u32. The value is uniform across a scope's body-entity run.

**Need.** We must know what the per-element u32 selects to write a Base Feature from a neutral model. It is `1` on one scope and `0` on every other, and it does not track the body count, so we cannot say what selects it.

### DR-26. Sketch-text fields

**Question.** What do these sketch-text fields hold?

- the thirty bytes of the class tail
- the flag byte after the horizontal-alignment enum and the flag byte after the vertical-alignment enum
- the five bytes between a `txt_tag` record's rotation and its colour components, the two bytes between its height and its anchor coordinates, and the eleven bytes between those coordinates and its text string, ten of them below class version 4
- the targets of the reference run a `txt_tag` record writes after its text string, the three bytes and eight unclassified bytes around its font weight, and the pairs of its leading block

**Known.** `f3d.md` §3.1 "Sketch text occupies two record classes" gives the two class GUIDs and the identity keys. `f3d.md` §3.1 "In a `textex_tag` record the property block" and `f3d.md` §3.1 "In a `txt_tag` record the twenty-nine bytes" give each legacy class's members up to the text string, including the anchor-point coordinates of the `txt_tag` class. `f3d.md` §3.1 "A `textex_tag` record writes two optional" and `f3d.md` §3.1 "A `textex_tag` record's class tail opens" give the remaining members and the placement transform. The indexed Design form is also specified there: its fixed header lane, ordinary property block, metrics, fixed suffix, and absence of a neutral anchor or rotation are settled. A `txt_tag` record's f64 directly after the property block is its stored rotation in radians about the anchor; zero is explicit. The legacy `textex_tag` form derives frame-text rotation and anchor from its transform. The colour components come from the run ahead of the font family. The decoder reads both legacy forms and the indexed Design form. A `txt_tag` record stores no width factor.

The thirty-byte class tail is `u32 0`, `u8 1`, five f32 `(0, 0, 0, 1, 1)`, `u32 0`, and `u8 0`. It is the same in every record of both classes. Its field boundaries are thus fixed, but no field in it changes, so no field in it has a meaning we can read. Of the three bytes after the horizontal-alignment enum, only the first is ever set. The single byte after the vertical-alignment enum is set when that byte is set, and clear when it is clear. The two are one flag written twice, or one flag and an echo of it. The alignment enums change independently of each other, so neither flag byte continues an alignment value. `f3d.md` §3.1 "Sketch text occupies two record classes" gives the leading block that both classes carry.

**Need.** We must know the meanings to write sketch text from a neutral model. A record that sets one of the two flag bytes and clears the other separates them. A class tail that differs from the constant above gives its fields a meaning. Nothing else in a sketch-text record changes with either.

### DR-27. Sketch-relation class-member meanings

**Question.** What do these members of a sketch-relation subclass hold?

- the three `u8` flags of the tangency class and the three of the rectangular-pattern class
- the u32-counted reference run of the rectangular-pattern class
- the u64 key and the u64 values of the pattern-table map, and the u32 of the pattern-table run
- the first of the two text-frame references
- the zero `u8` that closes the circular-pattern class

**Known.** `f3d.md` §3.1 "A sketch-relation class writes" gives the member sequence of each class, and each sequence closes the record on its exact end. The fields above have a width and no meaning. The decoder consumes them and transfers nothing from them.

**Need.** A pattern relation must be written back from a neutral model, and these members must carry the values the source would have written. The map keys and values are small integers in the range of record indices, so they may be a per-instance record grouping that a writer must rebuild rather than copy.

### DR-28. `VisibilityAttribute` values on display-scene sketch-curve nodes

**Question.** What design state does the `VisibilityAttribute` value of a display-scene sketch-curve node give?

**Known.** `f3d.md` §1.1 gives the `OGS.BlobFolder` family's role. The scene graph attaches attributes to each drawable node. An attribute is the length-prefixed attribute name, a five-byte prologue, and the attribute payload. The `VisibilityAttribute` payload is one byte with the value `0` or `1`. The attribute occurs on sketch-domain nodes and on work-geometry nodes; body, face, and edge nodes do not carry it. The value is `0` on every sketch-constraint, sketch-dimension, sketch-point, sketch, work-plane, work-axis, component, and group node. On a sketch-curve node the value is `0` or `1`. The value does not follow the curve type, the effect colour, the line style, or the owning sketch: one sketch holds curves with both values.

**Need.** The scene graph names the design entity of each node, so a sketch-curve node joins to a design curve record. If the value gives a curve property, that property has no other carrier and the scene graph is not a pure display cache. If the value gives a render-state decision, a decoder can drop the whole family. **Blocked on a specimen:** a document that hides one body or one sketch separates the two answers, and no such document is available to read.

**Note.** The attribute name does not give the meaning. Both direct readings of the value are inconsistent with the values on the other node kinds.

### DR-29. `EntityGenesis` values `0x4` and `0x8`

**Question.** What do the `EntityGenesis` values `0x4` and `0x8` mean?

**Known.** `f3d.md` §3.1 "A sketch-curve record contains" gives the origin bitfield and assigns bits `0x2`, `0x80`, and `0x100`. The values in the records available are `0x0`, `0x2`, `0x4`, and `0x8`. A document's sketch curves can carry `0x8` while some of their centre points carry `0x0`, and a sketch-text record carries `0x4` while its frame curves carry `0x0`, so neither value follows the record class or the owning sketch.

**Need.** A writer must emit the value the source would have written. Without the bit meanings, a neutral model cannot choose between `0x0`, `0x4`, and `0x8`.

### DR-30. `sketch_attrib_def` member `ref_b`

**Question.** What does `ref_b`, the second member of the `sketch_attrib_def` payload, name?

**Known.** `asm.md` §5.6 "`sketch_attrib_def` is source-link metadata" names it, gives its position in all three payload forms, and gives its range. It is zero in most links, and the decoder keeps a non-zero value as written. A non-zero value repeats across the links of one stream and is drawn from a small set per document. Where a document has sketch text, the values that recur most are the Design entity ids of its sketch-text records, and the `sketch_curve_id` beside them matches no sketch-curve record's persistent identity in that document. Where a document has no sketch text, non-zero values still occur and the `sketch_curve_id` beside them does match a stored sketch-curve identity. So the field names neither a sketch text in particular nor, by itself, the namespace `sketch_curve_id` is drawn from. One document spells it `18446744073709551615` throughout, the all-ones 64-bit pattern, where `0` is the value every other document writes for a link with no such reference.

**Need.** A writer must choose the value to emit for a link a neutral model does not carry. Without the meaning, only a link decoded from source restores it.

### DR-31. Other `Pipe` section and hollow forms

**Question.** Which primary-header values select square, triangular, and hollow `Pipe` sections? How is section size measured for the noncircular shapes, and where is hollow-wall placement encoded?

**Known.** Primary-header offset 29 value `1` selects a circular section and offset 30 value `1` selects a filled section. For that form, scalar ordinal two is the outside diameter and scalar ordinal three is an inactive positive thickness. The section is one filled disk and has no inner boundary. The settings reference contains a u32 and one finite double.

**Need.** A writer needs the selector values and dimension conventions for every supported generated section. Hollow forms also need the direction in which thickness changes the section boundary. **Blocked on specimens:** settling these forms needs otherwise equal pipes with each section shape and with the hollow option both off and on.

### DR-32. `Assemble` operand occurrence paths of the second container generation

**Question.** Where does the second container generation store the two `Assemble` operand occurrence paths, and how does each path join to its operand frame?

**Known.** `f3d.md` §3.1 "An `Assemble` scope stores two operand frames" gives the operand-frame layout of the 627-, 633-, 637-, 692-, 732-, and 772-byte forms, and `f3d.md` §3.1 "Except in the 772-byte class-261 form" gives the two occurrence-path records, their record indices five and two below the first operand construction in a four-owner scope, and 39 and 36 below it in an eight-owner scope.

The paired class tag does not select the frame layout. `f3d.md` §3.1 "A parameter scope is one logical indexed record" already states that both class tags are per-file dynamic values. Documents exist whose `Assemble` scopes pair with class 262 and whose 633- and 637-byte frames satisfy every fixed member of the 633- and 637-byte layouts: the marked operand references stand at scope offsets 24 and 164 and at 28 and 168, the two rigid row-major transforms stand at offsets 36 and 176 and at 40 and 180, and every zero-byte run between them holds zero. So frame length alone selects the layout.

The same documents do not hold the two path records. The record indices five and two below the first frame's construction record are absent from the Design stream, and no indexed record stands between the scope's paired header and that construction record. The occurrence GUIDs of those scopes instead occur as a run of length-prefixed UTF-16LE 36-character values at a 76-byte stride inside the region the paired header opens, which the record-header index does not enter.

**Need.** The projector accepts an alignment only when both the operand frames and the operand paths resolve, so an `Assemble` feature of this generation stays a native node even though its two transforms are readable. Widening the frame gate alone makes the scope fail validation instead, because the alignment then carries frames without paths. We must find the owning record and the count field of the GUID run, and the rule that assigns each run to one operand frame, before either layer can change.

### DR-32A. Component records that share one component GUID

**Question.** May two local component-occurrence carriers in one Design stream carry equal component GUIDs and unequal component-record references?

**Known.** `f3d.md` §3.1 "A local component occurrence is an indexed carrier" states that equal component GUIDs name the same reusable local component definition, and each carrier stores the same u64 component-record reference twice. The validator holds one component-record reference per component GUID per stream and reports a carrier that contradicts an earlier one.

Documents exist with two unplaced ordinal-one carriers whose component GUIDs are equal, whose occurrence GUIDs differ, and whose component-record references differ. Both carriers satisfy every fixed member of the 229-byte frame, and both duplicate their own component-record reference across offsets 24 and 197, so neither is a misread frame.

**Need.** The reading decides whether the component GUID or the component-record reference is the component definition's identity. If several records may describe one definition, the validator claim is too strong and the neutral component identity must come from the GUID alone. If not, one of the two carriers belongs to a second definition and the GUID is not an identity. Nothing yet separates the two readings, so the validator keeps the stronger claim and reports the second carrier.

### DR-33. Joined occurrences of a 772-byte class-261 `Assemble` scope

**Question.** What names the two joined component occurrences of a 772-byte class-261 `Assemble` scope?

**Known.** `f3d.md` §3.1 "An `Assemble` scope stores two operand frames" gives the form's two operand references, its two rigid connector transforms, and its ten owner lanes. `f3d.md` §3.1 "Except in the 772-byte class-261 form" states that this form stores no occurrence-path records. The decoder reads both connector transforms and the alignment angle and offset, and the validator accepts the form with frames and no paths.

A neutral assembly joint needs one occurrence per operand. Every other form supplies it from the first occurrence GUID of the operand's path record. This form has no path record, so each operand is identified only by the marked reference the frame stores, which names a construction record after the scope's paired header.

**Need.** The projector needs one occurrence identity per operand. Without it the feature stays a native node although its two connector frames and its alignment values are complete. Emitting a joint whose operands are empty would assert a join between unnamed bodies, so the operand identity has to come from the construction record the frame reference names, and that record's members are not resolved.

## 2. External references

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

**Known.** `f3d.md` §3.1 "**References.**" gives the cross-document form. The eight-byte value the item once asked about is the reference's target entity ID, and the ASCII GUID after it is the target record's type GUID; both resolve against the segment the reference's segment ID names in the target document. The UTF-16 asset GUID between them is one value per link and does not equal the target segment's own asset GUID.

**Need.** A writer must emit this GUID to build a cross-document reference, and we cannot derive it from either end of the link.

### XR-04. Occurrence-placement reference runs

**Question.** What do the counted reference run after the matrix, the modern tagged u32 run, and the two closing references of an occurrence placement name?

**Known.** `f3d.md` §1.4 "**Placement.**" gives the class, the layout, the instance discriminator, and the identity-marked matrix. The counted run reaches both local and cross-document targets. The two closing references name the same pair of entities for every placement of one document, so neither depends on the placement. A placement record can also carry the UTF-16 string `GatedByParent`; its position in the member sequence, its gate, and its value are not established.

**Need.** We must know the targets to write a complete occurrence placement. A reader takes the target path, the discriminators, and the transform without them.

## 3. Material assets

### MA-03. Distance unit-tag values

**Question.** What are the unit tags of a Distance value other than the three known length tags?

**Known.** `f3d.md` §3.2 "Boolean stores one u8." gives the tag structure `(quantity class << 12) | unit index`, with the unit index one-based, and gives three length tags: `0x200d` is centimetre, `0x200e` is millimetre, and `0x2016` is inch. The decoder converts these three to millimetres and returns no unit for every other tag. The tag is a property of the asset and does not track the document display length unit, so a change of that unit does not enumerate further tags. The schema `unit` attribute of a Distance property does not predict the tag, and one record mixes tags across its own members, so the attribute does not bound the tag set.

**Need.** A Distance with an unknown tag gets no unit. The neutral model then has a value with no scale. We must know which further unit indexes the length class `0x2` has, and whether a Distance takes a quantity class other than length.

### MA-04. Texture map-channel values

**Question.** What does each texture map-channel integer value mean?

**Known.** `f3d.md` §3.2 "`UnifiedBitmapSchema` and `BumpMapSchema` records" names the map-channel property. The application defines the value meanings.

**Need.** We must know the meanings to map a texture to the correct channel in a neutral model.

### MA-08. Omitted texture map-channel member

**Question.** Which member does a `TextureMap2dSchema` closure omit from its value block?

**Known.** `f3d.md` §3.2 "An `InstanceProperties` record opens with" states that such a block is one four-byte slot shorter than its closure, and that only omitting `texture_MapChannel_ID_Advanced` or `texture_MapChannel` leaves every surviving member at its schema default. The two are byte-degenerate: both are four-byte integers at adjacent positions, and the surviving pair reads `1` and `0` under either choice, which are the two members' declared defaults in either assignment.

**Need.** A writer must emit the same member set the reader expects, and the two choices shift every following member by four bytes. A texture asset authored with a map channel other than `1`, or with a non-default advanced channel id, separates them.

### MA-09. Face identity of a browser-node-reference appearance assignment

**Question.** How does a face-scoped appearance assignment in the browser-node-reference generation name its face?

**Known.** `f3d.md` §3.1 "A browser body record carries" gives the record and its body-scope form. A face-scoped record of this generation carries no physical-material preset name and no `299`-tagged head, so neither body-identity form applies. Its presentation envelope holds two lowercase GUIDs before the visual GUID. Neither GUID appears in the B-rep stream, and the stream carries no `NEUTRON_Material_attrib_def` attribute, so the §3.2 face appearance join has no operand. A document that assigns one face appearance writes two such records naming the same visual GUID, alongside one body-scope record.

**Need.** Face colour cannot transfer for this generation without the face operand. A document assigning distinct appearances to two named faces of one body separates the two GUIDs and fixes which one carries face identity.

### MA-05. Canvas visibility, mirroring, and crop

**Question.** Which Canvas fields hold the visibility state, the mirroring state, and the crop state?

**Known.** `f3d.md` §3.1 "A `Canvas` scope names" gives the Canvas geometry record. It holds the opacity, the plane frame, both boundary segments, the label, and the image asset. It names no visibility field, no mirroring field, and no crop field. The decoder reads the opacity and the plane frame only.

**Need.** A neutral canvas needs these three states to show the image correctly.

### MA-07. Precedence of library colour records

**Question.** What is the precedence of the `color-adesk-attrib` record and the `material-adesk-attrib` record against direct colours and appearance assignments? What do the twelve bytes and the eight bytes of a per-face assignment entry hold?

**Known.** `f3d.md` §3.2 "Color attribute records include" gives the content of both records. `color-adesk-attrib` holds a palette index. `material-adesk-attrib` holds a library lookup pair. `f3d.md` §3.2 "An explicit `rgb_color-st-attrib` or" gives the precedence of the two other colour records only. An explicit `rgb_color-st-attrib` or `truecolor-adesk-attrib` on a body or a face gives that target its neutral colour. If neither is present, one appearance binding with a base colour gives the colour. `f3d.md` §3.2 "Per-face appearance assignments live" gives the assignment entry; its two unnamed byte runs have a width and no meaning.

**Need.** A target can have more than one colour source. We must know the order to select one neutral colour, and the entry byte runs to write a per-face assignment from a neutral model.

## 4. T-splines

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

### TS-05. Compact Form cage-list tail

**Question.** What do bytes 49 through 99 of the compact one-cage cage-list record hold?

**Known.** `f3d.md` §1.1.1 "A compact one-cage cage-list record is 100 bytes" gives the indexed header, the ten zero bytes, the owning Form scope record index, the one-cage count, the sole cage-object record index, the two zero bytes, and the `0x00fc` member flags. The remaining 51 bytes are retained with the native record and have no assigned semantic field.

**Need.** A writer must know the compact-form tail before it can emit this record from a neutral Form feature. The decoder can bind the sole cage from the settled prefix and retain the tail for source fidelity.

## 5. Mesh geometry

### PM-01. `.paramesh` packed and per-triangle element contents

**Question.** We must find five answers:

- how a code-5 element packs three direction components into two f32 values
- what quantity a code-5 channel holds
- what a code-7 per-triangle value selects
- what the stream named by registry field 7 holds
- what descriptor `T` values other than `0`, `1`, and `3` select, and what registry fields 9 and 12 hold

**Known.** `f3d.md` §1.1.2 gives the container framing, both stream encodings, the descriptor value types, the registry channel entries, and the element codes. Every container declares one code-5 channel. Where the mesh is a cube, every f32 in that channel is `-1`, `0`, or `1`, which is the component set of the six face normals of a cube. A code-7 channel carries one zero per triangle while authored per-triangle colours instead add a code-4 channel, so a code-7 value is not that colour selector. Boolean descriptor `U = true` occurs on the code-5 channel and on no other.

**Need.** We must know the packing, the two channel contents, and the remaining descriptor and registry fields to write a container from a neutral model.

### PM-03. Corner order of an indexed `.paramesh` attribute channel

**Question.** Which triangle corner does each value of an indexed attribute channel belong to?

**Known.** `f3d.md` §1.1.2 gives the channel declaration, the element codes, and the count relation: a channel that declares an index stream holds its values deduplicated per vertex, and its index stream holds exactly the element count less the vertex count values. That relation holds for the code-2, code-4, and code-5 channels of every container, so the values are grouped by vertex and one value per vertex is implicit.

A code-4 channel stores authored corner colours exactly, in the sRGB scale and with alpha, and its element count equals the corner count when every corner carries a distinct colour. The stored order is a permutation of the corner order, not the corner order: where a mesh has six vertices, eight triangles, and twenty-four corners each carrying one of eight distinct colours, three consecutive stored triples equal the three corner colours of the first, second, and third triangle, and no grouping of the complete stored sequence into consecutive runs of four equals any vertex's corner colours. An index stream is monotonically increasing in steps of one and two, which is an offset array rather than an index array.

**Need.** The decoder transfers a channel with one value per vertex and reports every indexed channel. We must know the per-vertex corner order the index stream offsets address to transfer an authored corner colour or texture coordinate. A mesh with one vertex whose incident corners carry a known asymmetric colour cycle, and a second mesh with the same connectivity and one differing corner, would separate the fan order from the offset semantics.

### PM-02. Mesh Design-record classes without decoded content

**Question.** We must find two answers:

- what these five mesh-joined record classes hold:
  - `443807AD-8025-41A3-8A50-5157579C3D78` (add-in `ParaMesh`)
  - `6FC173DC-C7E3-402C-A8C0-891A26DADF8D` (add-in `ParaMesh`)
  - `E5B3F49A-D8D0-4EEF-BC2B-FCDDAEF9745E` (add-in `ParaMesh`)
  - `99F6967E-ED35-4222-B906-5CCF0AC70B53` (add-in `Fusion`)
  - `f85f2e62-7627-4922-a16d-53e1275d2aac` (add-in `Scene`)
- which of the two matrices of an `EA90DA22-556C-4C61-89BB-20C2681B7A9D` record governs the map from container coordinates to model space, and whether that matrix is the complete map

**Known.** `f3d.md` §3.1 "A mesh body's geometry container" gives the three decoded classes. Each of the five classes above occurs once per mesh body and does not occur in a document without one. The `EA90DA22-556C-4C61-89BB-20C2681B7A9D` record stores two equal affine matrices. Applying either matrix to container coordinates in model centimetres supplies the complete nonuniform scale and translation of a placed mesh. Separate mesh bodies retain separate containers, identities, and matrices even when their geometry bytes are equal.

**Need.** We must know the five payloads to write a mesh body from a neutral model, which duplicate matrix governs if they differ, and how a negative-determinant matrix affects triangle winding.

## 7. Test evidence

### EV-01. Typed feature projection reached only by a direct call

**Question.** Which scope kinds does the feature dispatcher promote to a typed definition when a real document supplies them?

**Known.** `crates/cadmpeg-codec-f3d/src/design/feature_project.rs` holds thirteen gates that promote a scope kind to a typed definition. Twelve are arms of one chain in `project_parameter_design_with_edge_identities` that tests `scope.kind` and falls through to `FeatureDefinition::Native`. The thirteenth is in `bind_form_cages`, which filters the scope list for kind `Form`. A kill test disabled each gate in turn, so that the scope fell through to a native record, and ran the complete f3d suite. Ten gates stayed green with the gate disabled: `JointOrigin`, `WorkPlane`, `WorkPoint`, `BaseFlange`, `RemoveBody`, `SurfaceStitch`, `CopyPaste`, `CopyPasteBodies`, `Base Feature`, and `Form`. Two gates have a test that reaches them through the dispatcher: `WorkAxis`, and the pair `SplitFace` and `DeleteFace`.

The projector leaves are tested. `crates/cadmpeg-codec-f3d/src/design/tests.rs` calls `project_remove_body` and `project_surface_stitch` and their siblings directly, with a scope value the test builds. No golden fixture under `crates/cadmpeg-codec-f3d/tests/golden/fixtures` carries any of the twelve untested scope kinds, so no decode golden pins the dispatcher path.

**Need.** A change to the scope-kind string, to the gate order, or to the record shape that carries the kind removes ten typed definitions with the suite green. We need one synthesized fixture per promoted kind, with a decode golden that pins the typed definition it produces.

### EV-02. Generate goldens that do not separate their inputs

**Question.** Which generate-writer behaviour do the `attributes`, `sketch_link`, and `topology_base` goldens each pin?

**Known.** `crates/cadmpeg-codec-f3d/tests/golden/generate` holds `attributes.bin`, `sketch_link.bin`, and `topology_base.bin`. The three are byte-identical and each is 2,055 bytes. Their source fixtures under `tests/golden/fixtures` differ from one another, and their decode goldens under `tests/golden/decode` differ from one another. The generate writer therefore maps three different inputs to one output, and two of the three names separate nothing.

**Need.** A reader of the golden tree counts three generate cases where one exists. We must find whether the generate lane is meant to discard what separates these inputs. If it is, two names must go or must state that they are duplicates. If it is not, the writer drops content that the decode side keeps.
