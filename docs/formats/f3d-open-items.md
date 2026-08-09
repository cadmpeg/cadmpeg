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

### DR-09A. Sheet-metal `Hem` fixed-section semantics

**Question.** What independent native settings do the four retained fixed-section fields and the indexed settings record carry?

**Known.** `f3d.md` §3.1 "A `Hem` scope names one parameter" gives the owner layouts, the header shift, the parameter source kinds, and the four retained fields. The owner layout separates rolled and teardrop inputs from the gap-and-length inputs. The executed transition separates the gap-and-length forms through its two coaxial bend carriers and supplies the signed direction from the preceding source plane; the settled invariants are in `f3d.md`.

The u32 at `85 + S`, u32 at `115 + S`, byte at `119 + S`, and u32 at `121 + S` are not the form, direction, or bend-position selectors. The gap-and-length, radius-and-angle, and gap-length-radius frames have distinct fixed-section lengths and rule-radius offsets.

**Note.** `f3d.md` §3.1 states that the form is carried by the parameter set and the executed transition and not by a fixed-section discriminator. `exact_hem_operation` in `design/decode/scopes.rs` does not receive the parameter source kinds. Its gap-and-length reader and its rolled reader share every gate — the reference count, the frame length, and the leading word — and differ only in whether the owner slots parse at offsets 42 and 53 or at 41 and 54. The form therefore comes from offset fit. The rolled reader's own comment concedes that the fixed frame proves the record identities only. The projector re-derives each role from the owner's parameter source kind, so a wrong form stays in the native arena and does not reach the neutral model.

**Need.** A source-preserving writer needs the independent settings carried by those fields and the indexed settings record.

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

**Known.** `f3d.md` §3.1 "An edge operand has" gives the result of these cases. An empty leading reference set identifies an unresolved edge-bearing operand. A disjoint set does the same. `f3d.md` §3.1 "For each selector, an" keeps an unresolved identity without a change. The `SurfacePatch` alternate two-clause recipe has a settled identity rule: its two clauses share one face-reference ordinal and one edge-reference ordinal; the shared face supplies a preceding boundary, and exactly one shared-edge candidate in that boundary resolves the operand.

**Need.** The neutral model needs one edge identity per operand for standard recipes whose reference, incidence, cardinality, or transition proofs do not resolve the candidate. Other alternate recipe tails still require their identity rules.

### DR-16. Extrude face-recipe candidate discriminator

**Question.** Which field selects one active B-rep face candidate when two rules both fail?

**Known.** `f3d.md` §3.1 "An `Extrude` face-group member" gives the two rules that work. The first rule applies when every effective active candidate has a support mapping and every mapping names the same non-empty predecessor-face set. The second rule uses a counted-boundary predecessor set.

**Need.** The neutral model needs one face per recipe member.

### DR-17. Extrude selection unknowns

**Question.** We must find four answers:

- what an identity that is absent from history denotes
- which field separates two profile loops that meet at the same ordered persistent Sketch points
- what the context UUID names
- what the optional slot of the fixed member tail holds

**Known.** `f3d.md` §3.1 "A nested entity-selection member" states that an identity absent from the preceding state gives no candidate. `f3d.md` §3.1 "An Extrude selection resolves" gives a fallback chain that ends in native retention. `f3d.md` §3.1 "The first identity-wrapper record" gives the presence encoding of the optional slot. The marker is zero when the slot is absent and one when the slot is present.

**Need.** Each remaining unknown makes one Extrude selection fall back to native retention. The neutral model then has no selection.

### DR-18. Extrude extent arbitration

**Question.** Which field determines the extent form when an extent discriminator and the stored termination reference disagree?

**Known.** `f3d.md` §3.1 "The extent form is carried by" gives the two per-side discriminators, their enum, and the parameter and reference set each value implies. Every implication holds in both directions, so no record separates the discriminator from the reference: the two never disagree. Extent value `2` uses either the role-`0x0000001100000000` face-termination form or the role-`0x0000000500000000` target-shape form. Extent value `3` does not occur, so neither its termination-entity search nor the tool-body extension mode of value `4` is exercised.

**Need.** A writer needs to know which field a reader follows before it can emit a record where the two differ, and whether value `3` may be written without a termination reference. A design authored with a to-object termination whose side is then switched to `to next` without clearing the object settles it.

### DR-19. Construction-group fields

**Question.** What do the construction-group scalar fields hold? What does the variant byte control? What do the group-role values outside the defined feature-specific sets mean? What does the boolean of the compact flag record a trailing reference names select?

**Known.** `f3d.md` §3.1 "Every `Extrude`, `Extrusion`, `Fillet`," gives the member order and the value limits. The group holds a nonzero u32 `ordinal`, a nonnegative finite f64 `scalar`, and a second copy of `ordinal` that one container generation omits. The value of `variant` is zero or one. The same paragraph defines the Extrude roles `0x05`, `0x08`, `0x41`, and `0x11` and defines Fillet role `0x04` as the full-round center-face form. In an Extrude one-sided to-entity extent, role `0x05` is a target-shape group whose members are whole-body recipe operands. In the Fillet full-round form the group has one compact persistent-identity member, one trailing compact flag whose boolean is `true`, and `variant = 0`; the member's bounded-face operand supplies the center face and the flag requests automatic side-face inference. Roles `0x81` and `0x100` name no defined operand family, and `0x100` does not fit in one byte. `scalar` is not equal to a compact-parameter value in the same feature scope, with or without unit scaling. `ordinal` is below 256, has one value for all groups of one feature scope, and does not decrease with the record index. The two optional references that follow the member run, and the count that opens the identity run, have no reader. The compact flag record has the automatic-side meaning in the Fillet full-round form; its meaning in other group families remains unsettled.

**Note.** The u32 word before `role` is zero in every record, so a reader that takes `role` as a u64 starting at that word and a reader that takes it as a u32 starting after it name the same value. The decoder takes the u64. Nothing separates the two readings.

**Need.** We must know the scalar, variant, optional-reference, and compact-flag meanings for the remaining construction-group forms before writing them from a neutral model.

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

**Known.** `f3d.md` §3.1 "Sketch text occupies two record classes" gives the two class GUIDs and the identity keys. `f3d.md` §3.1 "In a `textex_tag` record the property block" and `f3d.md` §3.1 "In a `txt_tag` record the twenty-nine bytes" give each legacy class's members up to the text string, including the anchor-point coordinates of the `txt_tag` class. `f3d.md` §3.1 "A `textex_tag` record writes two optional" and `f3d.md` §3.1 "A `textex_tag` record's class tail opens" give the remaining members and the placement transform. The indexed Design form is also specified there: its fixed header lane, ordinary property block, metrics, fixed suffix, and absence of a neutral anchor or rotation are settled. A `txt_tag` record's f64 directly after the property block is its stored rotation in radians about the anchor; zero is explicit. The legacy `textex_tag` form derives frame-text rotation and anchor from its transform. The colour components come from the run ahead of the font family. Horizontal alignment values `1`, `2`, and `3` mean left, right, and center. Vertical alignment values `1`, `2`, and `3` mean top, bottom, and middle. The decoder reads both legacy forms and the indexed Design form. A `txt_tag` record stores no width factor or alignment enums.

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

### DR-32A. Component records that share one component GUID

**Question.** May two local component-occurrence carriers in one Design stream carry equal component GUIDs and unequal component-record references?

**Known.** `f3d.md` §3.1 "A local component occurrence is an indexed carrier" states that equal component GUIDs name the same reusable local component definition, and each carrier stores the same u64 component-record reference twice. The validator holds one component-record reference per component GUID per stream and reports a carrier that contradicts an earlier one.

Documents exist with two unplaced ordinal-one carriers whose component GUIDs are equal, whose occurrence GUIDs differ, and whose component-record references differ. Both carriers satisfy every fixed member of the 229-byte frame, and both duplicate their own component-record reference across offsets 24 and 197, so neither is a misread frame.

**Need.** The reading decides whether the component GUID or the component-record reference is the component definition's identity. If several records may describe one definition, the validator claim is too strong and the neutral component identity must come from the GUID alone. If not, one of the two carriers belongs to a second definition and the GUID is not an identity. Nothing yet separates the two readings, so the validator keeps the stronger claim and reports the second carrier.

### DR-34. Sketch-curve geometry-family discriminator

**Question.** Which stored field selects the geometry family of a sketch-curve record?

**Known.** `f3d.md` §3.1 "A sketch-curve record contains" gives the circle-and-arc record class `F0130424-8B7E-4092-93C9-1CA807482534`. It names no class for a line and none for a NURBS. `decode_sketch_curve_identities` in `design/decode/sketch.rs` reads the record's class tag and does not resolve it against the segment type table. It selects the family with an ordered chain: legacy NURBS, NURBS, circular arc, line, compact planar line, then referenced analytic. The first grammar that accepts wins.

The arc grammar and the line grammar read the same twelve f64 at the same offset and give them different meanings. The arc grammar is first. It accepts when the second and fourth triples are unit vectors, the two are orthogonal, the radius is positive, and the angular interval is nonzero. A line record whose stored displacement has unit length and whose stored auxiliary direction is orthogonal to it satisfies the arc grammar. `f3d.md` §3.1 "A sketch-line geometry payload begins with" states that an imported sketch can keep a stale auxiliary direction, so the two conditions are not exclusive.

**Need.** The decoder must select the family from a stored field. A line that decodes as an arc gives the profile loop a wrong endpoint incidence, and the spatial-sketch classifier then puts the curve in the wrong branch. The line and NURBS record classes settle the item.

### DR-35. Live copy of a repeated record index

**Question.** Which copy of a record index is the live copy when a Design stream holds more than one copy that parses?

**Known.** `decode_sketch_texts`, `decode_sketch_points`, and `decode_sketch_curve_identities` in `design/decode/sketch.rs` each hold a set of emitted record indices and keep the copy that occurs first in byte order. The comment in `decode_sketch_texts` states the condition: "A stream can retain a superseded copy of a record beside the copy its index names, and both parse." `f3d.md` gives no rule that separates a superseded copy from the live copy.

**Need.** A stream that appends the current copy after the superseded copy makes every one of these records decode to the pre-edit content: a sketch text keeps its earlier string, height, and anchor, and a sketch point keeps its earlier coordinates. The decoder emits one record and records no loss, so no consumer can detect the substitution. The rule that marks the live copy settles the item. Until then the two copies must decode to equal content, or the record must be withheld with a loss.

### DR-36. Parameter-owner frame length

**Question.** What gives the serialized length of an indexed parameter-owner frame?

**Known.** `f3d.md` §3.1 "Every dimension or feature-input parameter has an indexed owner frame" gives the owner as a member sequence with an optional three-byte variant block. It gives no length. `decode_parameter_owners` in `design/decode/parameters.rs` supplies the length from the ordered list `[108, 107, 104, 103, 101, 100, 99]` and keeps the first length whose fixed-member checks hold. `parse_parameter_owner` then reads every field from a per-length offset table.

The owner is one logical indexed record delimited by two headers that carry the same record index, as `f3d.md` §3.1 "A parameter scope is one logical indexed record" states for a scope. The paired header therefore gives the frame end. `decode_parameter_owners` holds the complete header map and does not use it. `exact_fixed_scalar` in `design/decode/scopes.rs` measures the length from the record boundaries in the same way this function must.

**Need.** A frame whose length is not in the list produces no owner, no companion, no dimension frame, and no feature parameter, and no loss is recorded. A frame whose bytes satisfy a longer arm before the correct arm shifts every field offset and gives the parameter and companion joins a wrong record index. The validator compares the owner value against the parameter value and catches the second case only. `f3d.md` §3.1 "The standard `Loft` scope stores its result operation" names a 105-byte scalar frame that the list does not hold.

### DR-37. Extent carrier of the current Extrude prologue

**Question.** Which per-side words carry the extent form of a current-generation `Extrude` prologue?

**Known.** `f3d.md` §3.1 "An `Extrude` or `Extrusion` scope stores its result-operation" states that the two u32 after the operation are the travel direction and the face-extend option and that "they are not an extent pair". It gives direction values `1 = one side`, `2 = two sides`, and `3 = symmetric`. `f3d.md` §3.1 "The extent form is carried by" puts the two per-side extent words after the profile normal and the scope reference slots, with values `0 = absent`, `1 = distance`, and `2 = to entity with an offset`.

**Conflict.** `exact_current_extrude_prologue` in `design/decode/scopes.rs` names the pair at `operation + 4` and `operation + 8` `extent_discriminators` and takes the extent form from it: `[1, 1]` gives a one-sided to-face extent, `[1, 2]` a one-sided distance, `[2, 0]` a two-sided distance, and `[3, 2]` a symmetric distance. The first element of each pair reproduces the direction enum the specification gives. The mapping of the second element also opposes the extent enum, because it takes `1` as a to-face extent where the extent enum gives `1` as a distance. `exact_legacy_shifted_extrude_prologue` reads the same pair, names it `direction_face_extend_values`, and takes the extents from separate per-side offsets. `validate.rs` repeats the same four pairs, so the validator confirms the decoder and not the format.

**Need.** The two readings disagree about which words carry the extent. A one-sided blind extrude whose face-extend option is `1` decodes as a to-face extrude, and the projector then requires a termination group that the record does not hold. The decision names the correct offsets for the current form.

### DR-38. Extent of the legacy-distance Extrude dialect

**Question.** Which field carries the extent form of the legacy-distance `Extrude` prologue?

**Known.** `exact_legacy_distance_extrude_prologue` in `design/decode/scopes.rs` reads one word at `operation + 4` and refuses the record when the value is not `2`. Under `f3d.md` §3.1 "An `Extrude` or `Extrusion` scope stores its result-operation" that word is the travel direction and the value `2` is two sides. `DesignExtrudePrologue::extent` in `records.rs` returns a one-sided distance for this dialect without reading a field.

**Need.** The projector emits a one-sided blind extent from that value. If the word is the travel direction, the second side is dropped and no loss is recorded. The field that carries the extent of this dialect settles the item.

### DR-45. Face set of an Extrude to-shape target

**Question.** Which faces of a whole-body recipe target define the shape an Extrude terminates on?

**Known.** `f3d.md` §3.1 "The extent form is carried by" gives the role-`0x0000000500000000` target group whose members are whole-body recipe operands. It gives no rule that turns the group into a neutral face set. `f3d.md` §3.1 "A `Combine` scope stores" uses the candidate faces of a body recipe in the opposite direction, as a check on a body identity: "if candidate faces are present, that body must occur in their body incidence".

`resolved_body_recipe_shape` in `design/face_resolve.rs` collects the union of every reference's candidate faces and emits it as the resolved face set of the termination. A reference's candidate faces are the active faces whose persistent subentity tag holds that reference. The set is not proved to be the body's complete boundary and is not proved to be the termination surface.

**Need.** A recipe reference that tags three faces of a twelve-face body makes the neutral model state that the extrusion terminates on those three faces. The rule that gives the target shape from a whole-body recipe settles the item.

### DR-46. Join from a `Chamfer` dimensional specification to its edge group

**Question.** Which stored field joins a `Chamfer` dimensional specification to one construction-operand group?

**Known.** `f3d.md` §3.1 "Within a scope of the Chamfer family," states that the groups in scope-reference order pair one to one with the specifications in increasing owner-local order. `project_chamfer` in `design/feature_project.rs` implements that rule with a positional zip of the two sorted lists.

The `Fillet` family has a stored join for the same relation. `DesignFilletRadiusLaw` carries the parameter record index of each radius, and `project_fillet_arm` joins on it.

**Need.** The `Chamfer` join is positional and the `Fillet` join is not. Either the `Chamfer` record holds an equivalent stored index that the decoder does not read, or the positional rule is the format's rule. A scope whose owner-local order does not follow the scope-reference order separates the two.

### DR-47. Recipe-kind conditioning of a recipe-backed linear dimension

**Question.** Does the recipe kind of a dimension companion's records restrict the candidate family of a recipe-backed linear dimension?

**Known.** `f3d.md` §3.1 "A recipe-backed linear dimension" gives the candidate rule and states that candidates are axis-aligned point pairs and parallel line pairs. It states no condition on the recipe kind.

`design/dimensions.rs` adds one. When every recipe record of the companion has the `Edge` kind, the candidate list keeps only the parallel-line-pair results and drops the point-pair results. The filter removes a rival candidate instead of proving it wrong: a companion whose candidates hold one point pair and one line pair of equal measurement gives one surviving candidate and the dimension asserts it.

**Need.** The filter is in neither `f3d.md` nor this document. Either the recipe kind carries the candidate family, or the two candidates must give a repeated dimension or native retention.

### DR-48. Adjacent-versus-span discriminator of a rectangular sketch pattern

**Question.** Which stored member states whether a rectangular sketch-pattern distance is adjacent spacing or the seed-to-final span?

**Conflict.** `f3d.md` §3.1 "The two reference runs hold the same members." states that the member is stored: "A non-empty counted reference run stores adjacent spacing in the source distance scalar. An empty counted reference run stores the total seed-to-final span." DR-27 states of the same run that the field has "a width and no meaning" and that the decoder transfers nothing from it. `exact_rectangular_pattern` in `design/constraints.rs` reads neither: it builds the instances under both readings and keeps the reading that gives a unique result.

**Known.** The two readings agree for a count of two, because the span divided by count minus one equals the adjacent spacing. Both arms then give equal directions and equal instances and differ only in which parameter slot holds the reference. The uniqueness gate fails and every two-instance rectangular pattern falls back to a native relation.

**Need.** The three statements disagree. The reading decides whether the decoder must retain the counted run's emptiness and whether a two-instance pattern can transfer at all.

### DR-49. Verification of a spatial counted-offset pair

**Question.** What proves the offset distance and the offset side of a spatial offset pair whose curves are not lines?

**Known.** `f3d.md` §3.1 gives the counted offset relation. `spatial_counted_offset_dimension_definition` in `design/dimensions.rs` measures a pair only when both curves are lines. A pair of circles, arcs, or NURBS bypasses the distance check, enters the emitted pair list, and takes the reversal flag that a line pair elsewhere in the same relation set. One witnessed pair legitimizes every other pair.

The planar sibling `exact_counted_offset` in the same file is stronger. It requires a measured offset for every pair and holds an arc-specific rule.

**Need.** A relation with one line pair and three arc pairs states an offset distance and an offset side for the three arc pairs that nothing measured. The measurement rule for a non-line spatial offset settles the item.

### DR-50. Construction order of scopes with no history-state identity

**Question.** What gives the construction order of two feature scopes that carry no history-state identity?

**Known.** `f3d.md` §3.1 "A parameter scope is one logical indexed record" gives a partial order only: "When one scope's preceding identity equals another scope's current identity, the former follows and depends on the latter." A suppressed `Extrude`, `Fillet`, or `Chamfer` and every `Sketch` carry no such identity and contribute no edge.

`assign_feature_ordinals` in `design/feature_project.rs` seeds each feature's ordinal from the scope's byte offset and uses it as the tie-break of the topological sort. The neutral `Feature.ordinal` is documented as the stable construction order within the source history.

**Need.** The neutral model states a construction sequence that the file does not state for every state-null scope. The field that carries the timeline position settles the item.

### DR-51. Authored order of design configuration variants

**Question.** Where does a document store the authored order of the variants of a configuration table?

**Known.** `f3d.md` gives the configuration tables as JSON documents. `design/configurations.rs` reads the variants from a `serde_json::Map`. No crate in the workspace enables the `preserve_order` feature of `serde_json`, so that map is a `BTreeMap` and its iteration order is the ASCII order of the variant names. The projector derives `NeutralConfiguration.ordinal` from that iteration and from a counter that runs across every table. `cadmpeg-ir` documents the field as the position in the design configuration list.

**Need.** A table authored as `Small`, `Medium`, `Large` is exported with `Large` at ordinal zero. Either the JSON document holds the authored order in a member the decoder does not read, or the order is not recoverable and the neutral model must not state one.

### DR-52. Location of the ACT table and the root-component discriminator

**Question.** Which stored reference locates the `ACTTable` record, and which field marks the root component's link?

**Known.** `f3d.md` §3.1 gives the per-component links and states that "the record whose entity key names entity 3 is the root component's". `decode_root_components` in `act.rs` applies no entity-3 test and emits every link as an `ActRootComponent`, whose fields are documented as the document root entity and the document display name. An N-component document therefore holds N records that each claim to be the document root.

`decode_table` in the same file locates the table by the first `ACTTable` byte window in the stream and returns an empty entity table and an empty GUID pool from five distinct rejection paths. No loss is recorded at either call site, so a document whose first window is not the table decodes to an ACT arena that has lost its table half in silence.

**Need.** The entity-3 rule is in the specification and not in the decoder. The table's stored location, and a per-record mark for the root link, settle both halves.

### DR-53. ACT entity identity when one record index carries two entity ids

**Question.** What identifies an ACT entity when one record index carries more than one entity id?

**Known.** `decode` in `act.rs` keys its entity map on the pair of record index and entity id, so the code states that one record index can carry two ids. `crate::ids::native_scoped_id` builds the emitted identity from the record index alone. Two arena records then carry equal identities. The channel assignment overwrites rather than merges, so the later scan position wins.

**Need.** The identity is the join key for annotations and for the writer's patch lookup. Two records with one identity make the patch target ambiguous. Either the record index is the identity and the map key is too wide, or the entity id is part of the identity and the emitted id is too narrow.

### DR-54. Selection of the design asset folder

**Question.** Which manifest member names the design asset folder?

**Known.** `f3d.md` §1.3 states that a document can hold sibling asset folders with independent GUIDs and asset types, and that the manifest holds the asset-folder name run with the design folder first in the counted run. `scan` in `container.rs` does not read that run. It takes the folder of the first archive entry whose name holds `Breps.BlobParts` and keeps it.

The value filters the Design stream for the occurrence binder, the recipe and parameter decoders, two body binders, and the T-spline reader. A document whose first B-rep-bearing entry belongs to another asset folder makes every one of those passes match no stream, and the decode then reports an empty design model with no error. A document with no B-rep leaves the value absent and the filter widens to every folder whose name holds `Design`.

**Need.** The manifest run is the stored authority and is not read. The two regimes above differ and neither is stated.

### DR-55. Localized names of the edge-treatment families

**Question.** Which localized scope-kind names does the edge-treatment group-retention rule cover?

**Known.** `f3d.md` §3.1 names `Congé` and `Chanfrein` as the localized forms that do not require every selection to use a counted group. `extend_related_design_records` in `decode.rs` matches those two strings. `design_feature_family` in `design/mod.rs` is the crate's localization map and holds `Fillet`, `Congé`, `Abrundung`, and `Arredondamento` for one family and `Chamfer` and `Chanfrein` for the other.

**Need.** A German or Portuguese `Fillet` scope keeps construction-operand groups that the French document drops, so the decode output depends on the authoring language. The complete localized name set settles the rule for both documents.

### DR-56. Mask width of a sketch-relation state word

**Question.** Which class member gives the width of a sketch-relation's stored state mask?

**Known.** `relation_has_paired_member_run` in `design/decode/sketch.rs` reads a discriminator byte and returns no answer when the leading scan fails or when the byte is neither zero nor one. `validate_sketch_relation_edits` in `writer/patch/edits.rs` then assumes the wide form and writes eight bytes at the state offset. A record that stores the u32 form loses the four bytes after the mask, which are the next member of the record.

**Need.** The writer substitutes a width for an unresolved discriminator instead of refusing. The class member that gives the width settles the item.

### DR-57. Members of a generated presentation envelope and browser-node join

**Question.** Which GUID of a presentation envelope joins to a browser-node record, and which members must a generated envelope hold?

**Known.** `f3d.md` §3.1 "A browser body record carries" states that exactly one GUID in the presentation envelope equals a browser-node record's GUID and that the node record carries the body's Design entity suffix. `encode_design_bulkstream` in `writer/generate/records.rs` writes the literal string `Body` and the all-zero GUID `00000000-0000-0000-0000-000000000000` into the envelope, and separately synthesizes each browser-node GUID from the body's position in the neutral body list as `00000000-0000-0000-0000-{ordinal:012X}`. The envelope GUID and the node GUID therefore never agree, and a re-ordering of the neutral body list changes every emitted node identity.

The decoder does not use the documented structure either. `decode_design_assignments` in `materials.rs` searches backward up to ten strings for an entity id and forward up to fifteen strings for the library marker, and takes the string before that marker as the visual GUID. The two invented strings satisfy that search.

**Need.** A generated document holds a browser-node set and an appearance envelope that the documented join cannot connect. The envelope's real member sequence, and the source of a browser-node GUID, settle what a writer must emit.

### DR-58. Bound of a Design body-map pair count

**Question.** What bounds the pair count of a Design body map?

**Known.** `f3d.md` §3.1 gives the body map as a count followed by that many sixteen-byte pairs. It gives no bound. `body_bindings` in `design/decode/body.rs` tries the counts one through 64 in ascending order and keeps the first count whose stored word equals the trial. The comment states the reason the ascending scan is taken to be unambiguous: the high halves of the little-endian ids are zero. That statement is an observation of the values present and not a checked invariant.

**Need.** A blob with more than 64 pairs produces no binding at all, and every body in it loses its Design binding, its visibility, and its material assignment with no loss recorded. An entity suffix at or above 2^32 whose high word equals a trial count makes the scan accept the wrong count and mis-pair every key.

### DR-59. Body visibility when two browser-node records name one entity suffix

**Question.** Which browser-node record gives the visibility of a body when two records name one entity suffix?

**Known.** `browser_node_records` in `design/decode/body.rs` is an unanchored byte scan: every offset that holds a length word, a 36-character GUID, a zero-or-one byte, the pair `01 01`, and a u64 is accepted. `browser_node_hidden_flags` collects the results into a map, so the last scan hit wins in silence. `browser_node_entities` reads the same record set, detects the same collision, and drops the entity instead of keeping one.

**Need.** The two functions read one record set and resolve the same ambiguity in opposite ways. A body is exported hidden when it is shown, with no loss recorded. The record that owns the visibility of an entity suffix settles the item.

### DR-60. Member order of a spline-group constraint

**Question.** Which reference run gives the member order of a neutral spline-group constraint?

**Conflict.** `f3d.md` §3.1 "The two reference runs hold the same members." states that "Only the second run is in semantic order" and that "The first run holds the same members in an unrelated order, so its last entry does not name the spline". `project_spatial_sketch_constraints` in `design/sketch_project.rs` builds the neutral spline-group members from the first run. `constraints.rs` makes the same choice for a planar sketch. The control-polygon inference earlier in `sketch_project.rs` uses the second run and states the rule in its comment.

**Known.** `cadmpeg-ir` documents the neutral field as the ordered spline-group members. The validator checks the member count only.

**Need.** A spline group of three or more members carries an order the specification calls unrelated, and its last entry is not the spline. A consumer that reads the last entry as the spline curve, or that rebuilds the control polygon from adjacent pairs, gets the wrong entity.

### DR-61. u64 values in a cross-document `Combine` selector tail

**Question.** What do the two u64 values around the fixed `u32 48` in a cross-document `Combine` body-selector tail mean?

**Known.** `f3d.md` §3.1 "A tool body-selection record" gives the complete selector grammar and the independent occurrence, external-body, segment, asset, link, property, and version fields. The first u64 follows u32 `9` and u16 `2`. The second follows u32 `48`. The two values are retained in source order and can differ between selectors that share the same owning scope.

**Need.** Their meanings determine whether they participate in persistent body identity and how a writer derives them from a cross-document body selection.

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

### XR-05. Precedence of a role-adjacent carrier against a placement record

**Question.** Which carrier gives the occurrence transforms of an external reference when a Design stream holds both a class-256 role-adjacent carrier and one or more placement records for one role?

**Known.** `f3d.md` §3.1 states that in class-256 carriers the placement occurrence is an LP-UTF16 occurrence-role GUID, two zero bytes, and a rigid transform equal to the scope transform. `f3d.md` §1.4 "**Placement.**" gives the `CE2913AA` placement record and states that several placements may carry the same role and place the same target document more than once.

`bind_occurrences` in `xref.rs` prefers the carrier form for a whole stream. It scans the stream for role-adjacent transforms, and when that scan returns any result it discards every structured placement of that stream. The scan applies no class-tag gate, does not check the two zero bytes, and accepts every needle hit in every indexed record whose following window decodes as a rigid matrix.

**Need.** A stream that holds both forms loses the placement count and the placement transforms. Components then appear at the wrong pose or in the wrong number, and the merged `.f3z` document repeats the error. The precedence, and the gate that limits the carrier scan to class-256 records, settle the item.

### XR-06. Gate of the placement tagged run

**Question.** Which stored field gates the tagged u32 run of an occurrence placement?

**Known.** `f3d.md` §1.4 "**Placement.**" states that the tagged u32 run and its reference occur only where the meta stream's serializer magic is `1234`. `placement_tail` in `xref.rs` does not read that magic. It takes the run to be present when the next byte is neither zero nor one, which is the presence encoding of a reference.

The crate reads the same magic elsewhere: `design/decode/sketch.rs` selects a header width from it, and `ids::design_segment` reaches the sibling meta stream.

**Need.** A modern-container placement whose tag byte is zero or one takes the legacy branch, the record does not close on its end, and the placement is dropped. The stored discriminator is available and unread.

### XR-07. Absent occurrence transform against the identity placement

**Question.** How does a reader separate an occurrence that stores no transform from an occurrence whose placement did not decode?

**Known.** `f3d.md` §1.4 states that an external occurrence without a serialized transform places the target document unchanged. `project_occurrences` in `xref.rs` substitutes the identity matrix for an absent transform. The absence has two causes: the record carried the identity marker, which is the documented form, and the placement did not decode or no placement named the role.

The `.f3z` merge writes the note `(identity placement)` for the first cause. The plain `.f3d` path writes nothing, and `DesignProjectionGaps` holds no counter for a missing occurrence transform.

**Need.** A component placed at the origin because its placement failed is indistinguishable from a component the document places at the origin. The decode must record a loss for the second cause.

## 3. Material assets

### MA-03. Distance unit-tag values

**Question.** What are the unit tags of a Distance value other than the three known length tags?

**Known.** `f3d.md` §3.2 "Boolean stores one u8." gives the tag structure `(quantity class << 12) | unit index`, with the unit index one-based, and gives three length tags: `0x200d` is centimetre, `0x200e` is millimetre, and `0x2016` is inch. The decoder converts these three to millimetres and returns no unit for every other tag. The tag is a property of the asset and does not track the document display length unit, so a change of that unit does not enumerate further tags. The schema `unit` attribute of a Distance property does not predict the tag, and one record mixes tags across its own members, so the attribute does not bound the tag set.

**Note.** The neutral model does not have a value with no scale. `distance_property` in `materials.rs` returns no value for an unknown tag, and every caller in `texture_asset` then substitutes `0.0`. A bump depth or a real-world scale authored in an unknown length unit is therefore exported as zero, which is also the unset value, and no loss is recorded. The item's Need and the decoder disagree.

**Need.** A Distance with an unknown tag gets no unit. The neutral model then has a value with no scale. We must know which further unit indexes the length class `0x2` has, and whether a Distance takes a quantity class other than length.

### MA-04. Texture map-channel values

**Question.** What does each texture map-channel integer value mean?

**Known.** `f3d.md` §3.2 "`UnifiedBitmapSchema` and `BumpMapSchema` records" names the map-channel property. The application defines the value meanings.

**Need.** We must know the meanings to map a texture to the correct channel in a neutral model.

### MA-08. Omitted texture map-channel member

**Question.** Which member does a `TextureMap2dSchema` closure omit from its value block?

**Known.** `f3d.md` §3.2 "An `InstanceProperties` record opens with" states that such a block is one four-byte slot shorter than its closure, and that only omitting `texture_MapChannel_ID_Advanced` or `texture_MapChannel` leaves every surviving member at its schema default. The two are byte-degenerate: both are four-byte integers at adjacent positions, and the surviving pair reads `1` and `0` under either choice, which are the two members' declared defaults in either assignment.

**Note.** The decoder has already taken one of the two choices and exports the result. `instance_property_serializes` in `protein.rs` omits `texture_MapChannel_ID_Advanced`, and its own comment states that the choice is not decidable from the bytes. `texture_asset` in `materials.rs` then reads the surviving first word as `texture_MapChannel` and puts it in the neutral `map_channel`. If the omitted member is the other one, that neutral value is the advanced channel id and the texture binds to the wrong UV set. The record consumes exactly, so the exact-consumption check cannot separate the two.

**Need.** A writer must emit the same member set the reader expects, and the two choices shift every following member by four bytes. A texture asset authored with a map channel other than `1`, or with a non-default advanced channel id, separates them.

### MA-05. Canvas visibility, mirroring, and crop

**Question.** Which Canvas fields hold the visibility state, the mirroring state, and the crop state?

**Known.** `f3d.md` §3.1 "A `Canvas` scope names" gives the Canvas geometry record. It holds the opacity, the plane frame, both boundary segments, the label, and the image asset. It names no visibility field, no mirroring field, and no crop field. The decoder reads the opacity and the plane frame only.

**Need.** A neutral canvas needs these three states to show the image correctly.

### MA-07. Precedence of library colour records

**Question.** What is the precedence of the `color-adesk-attrib` record and the `material-adesk-attrib` record against direct colours and appearance assignments? What do the twelve bytes and the eight bytes of a per-face assignment entry hold?

**Known.** `f3d.md` §3.2 "Color attribute records include" gives the content of both records. `color-adesk-attrib` holds a palette index. `material-adesk-attrib` holds a library lookup pair. `f3d.md` §3.2 "An explicit `rgb_color-st-attrib` or" gives the precedence of the two other colour records only. An explicit `rgb_color-st-attrib` or `truecolor-adesk-attrib` on a body or a face gives that target its neutral colour. If neither is present, one appearance binding with a base colour gives the colour. `f3d.md` §3.2 "Per-face appearance assignments live" gives the assignment entry; its two unnamed byte runs have a width and no meaning.

**Need.** A target can have more than one colour source. We must know the order to select one neutral colour, and the entry byte runs to write a per-face assignment from a neutral model.

### MA-09. Face identity of a browser-node-reference appearance assignment

**Question.** How does a face-scoped appearance assignment in the browser-node-reference generation name its face?

**Known.** `f3d.md` §3.1 "A browser body record carries" gives the record and its body-scope form. A face-scoped record of this generation carries no physical-material preset name and no `299`-tagged head, so neither body-identity form applies. Its presentation envelope holds two lowercase GUIDs before the visual GUID. A document that assigns one face appearance writes two such records naming the same visual GUID, alongside one body-scope record.

**Note.** This item was closed by `4ae4944c9` and is reopened. That commit takes the GUID immediately before the visual GUID as the face identity, writes the choice into `f3d.md` §3.2 "In the paired-library browser-node-reference generation," as a rule, and pins it with a test built from fabricated GUIDs that encodes the same choice. The item states that two lowercase GUIDs stand before the visual GUID and that nothing separates them, and its Need names the specimen that separates them. The commit adds no such specimen, so the rival GUID is discarded and not disproved. Writing the choice into the specification is not evidence for it.

The closure also has no operand. `resolve_face_appearance_bindings` in `decode.rs` builds its face map only from face attributes that hold `NEUTRON_Material_attrib_def`, and this item states that the stream of this generation carries no such attribute. Either that statement is wrong, or the closed rule binds nothing in the generation it settles. The commit resolves neither reading.

**Need.** Face colour cannot transfer for this generation without the face operand. A document assigning distinct appearances to two named faces of one body separates the two GUIDs and fixes which one carries face identity. The same document also shows which attribute carries the face-side join.

### MA-10. Precedence of the base-colour members of one schema record

**Question.** Which member gives the base colour when one appearance record holds more than one colour member?

**Known.** `f3d.md` §3.2 "An explicit `rgb_color-st-attrib` or" gives the precedence of the colour records against appearance bindings. It gives no precedence among the colour members inside one record. `appearances_from_schema_records` in `materials.rs` takes the first member that resolves from the ordered list `generic_diffuse`, `opaque_albedo`, `surface_albedo`, `common_Tint_color`.

`common_Tint_color` is a `CommonSchema` member and stands beside `surface_albedo` on a Prism record. The list prefers `surface_albedo` and drops the tint with no loss recorded.

**Need.** The order is the only arbiter and is not in the specification. A tinted Prism appearance carries both members, and the neutral colour must come from the member the source uses.

### MA-11. Owner join of a Design material assignment

**Question.** Which stored reference joins a Design material-assignment token to its target entity and to its visual GUID?

**Known.** `f3d.md` §3.2 gives the join backbone through the numeric design-entity namespace. It gives no adjacency rule. `decode_design_assignments` in `materials.rs` searches the ten strings before the material token for the nearest one that parses as `<name>_<digits>`, and the fifteen strings after it for the library marker, and takes the string before that marker as the visual GUID.

`entity_suffix` accepts any string of that shape, so an unrelated stored name such as a texture slot or a component label between the true entity id and the token retargets the assignment. The three window sizes are in neither `f3d.md` nor `docs/layouts/f3d.toml`, and a record with one extra string in the run drops the assignment with no loss recorded.

**Need.** The record's own framing is settled — `f3d.md` §3.2 "Per-face appearance assignments live" gives the class, the count, and the entry width — and the decoder does not use it. The stored reference from a token to its entity settles the item.

### MA-12. Appearance identity of an assignment with no preset

**Question.** What distinguishes two body appearance assignments that carry no preset name and whose visual GUIDs name no catalog asset?

**Known.** `decode_with_bodies` and `bind_bodies` in `materials.rs` accept an appearance for an assignment when the visual GUIDs agree or when the assignment's preset name equals the appearance's name. A decoded appearance always carries a name; a synthesized one carries the assignment's preset, which is absent unless the string begins with `Prism-`.

Two preset-less assignments whose GUIDs resolve to no asset therefore compare equal on the name clause, because both names are absent. The first synthesizes one appearance and the second finds it, so the second GUID produces no appearance and no loss. Both bodies then bind to the first appearance.

**Need.** The name clause exists for the preset-named case. The identity of a preset-less assignment must come from its visual GUID alone, or two bodies with different appearances are reported as sharing one.

### MA-13. Body identity when several bodies carry one key

**Question.** Which body does a material assignment name when more than one body carries its ASM body key, or when two body keys carry one entity suffix?

**Known.** `body_for_key` and `decode_body_map` in `materials.rs` both state the multiplicity in their own comments and both resolve it by taking the smallest identity, for a stable digest across process runs. `resolve_body_selector` in `brep.rs` meets the same shape and raises `Malformed` instead.

**Need.** Determinism is not correctness. The other bodies keep no appearance binding and no loss is recorded. The field that separates the bodies settles the item; without it the two paths must agree on refusing.

### MA-14. Length of a visual GUID and the uniqueness of its match

**Question.** Is a visual GUID exactly 36 characters, and may two appearances agree on their first 36 characters?

**Known.** `visual_guid_matches` in `materials.rs` compares only the first 36 characters and `is_guid_prefix` accepts a longer string. `resolve_face_appearance_bindings` in `decode.rs` then takes the first appearance that matches, with no uniqueness gate, so two appearances that agree on the prefix bind by arena order. `f3d.md` §3.2 gives the library-qualified preset form, in which a suffix follows the GUID.

**Need.** The comparison is deliberately truncated and the selection is not gated. The suffix's role, and whether it separates two assets, settle whether the truncation is safe.

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

**Note.** The decision to withhold the scalar is correct, but its result is not recorded. `tsm.rs` emits every `SubdEdge` with `sharpness` `[0.0, 0.0]` and `sector_coefficients` `[0.0, 0.0]` while setting `tag` to `Crease` from the `ec` records. A creased edge therefore carries a tag that says crease and a numeric sharpness that says smooth, and the only loss the reader emits for this family is the unknown-record count. A consumer that reads the numeric field renders every crease smooth.

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

### PM-03. Join from a mesh geometry container to its feature scope

**Question.** Which stored reference joins a mesh body's geometry container to the `Base Mesh Feature` scope that owns it?

**Known.** `f3d.md` §3.1 "A mesh body's geometry container" gives the mesh-body record and its marked reference to the scope's record index. `marked_record_indices` in `design/decode/mesh.rs` does not read one reference. It scans every byte offset of the payload for the pattern of a marker byte, eight bytes, and two zero bytes, and keeps every hit. `decode.rs` then registers the tessellation under every hit.

`decode_mesh_bodies` reaches the container through two chained first-match searches: the first record whose bytes hold the GUID, then the first record in the same stream whose bytes hold the reference and whose two stored matrices decode. Neither search proves that no second record qualifies.

**Need.** A document with two mesh bodies whose byte windows collide gives one feature two tessellations. The single stored reference the specification names must be read at its own offset.

## 7. Test evidence

### EV-01. Typed feature projection reached only by a direct call

**Question.** Which scope kinds does the feature dispatcher promote to a typed definition when a real document supplies them?

**Known.** `crates/cadmpeg-codec-f3d/src/design/feature_project.rs` holds thirteen gates that promote a scope kind to a typed definition. Twelve are arms of one chain in `project_parameter_design_with_edge_identities` that tests `scope.kind` and falls through to `FeatureDefinition::Native`. The thirteenth is in `bind_form_cages`, which filters the scope list for kind `Form`. A kill test disabled each gate in turn, so that the scope fell through to a native record, and ran the complete f3d suite. Seven gates stayed green with the gate disabled: `BaseFlange`, `RemoveBody`, `SurfaceStitch`, `CopyPaste`, `CopyPasteBodies`, `Base Feature`, and `Form`. A synthesized dispatcher test now exercises the first six of these gates with their typed operation records. A synthesized archive-scan test exercises the `Form` cage-binding gate. The dispatcher also has synthesized end-to-end tests for `JointOrigin`, `WorkPlane`, and `WorkPoint`; `WorkAxis` was already covered. The pair `SplitFace` and `DeleteFace` also has dispatcher coverage.

The projector leaves are tested. `crates/cadmpeg-codec-f3d/src/design/tests.rs` calls `project_remove_body` and `project_surface_stitch` and their siblings directly, with a scope value the test builds. The synthesized dispatcher test also checks the six typed definitions returned by the complete projector. `crates/cadmpeg-codec-f3d/src/tests.rs` supplies a minimal Design stream to the `Form` archive-scan gate and checks the resulting typed definition. No golden fixture under `crates/cadmpeg-codec-f3d/tests/golden/fixtures` carries `BaseFlange`, `RemoveBody`, `SurfaceStitch`, `CopyPaste`, `CopyPasteBodies`, `Base Feature`, or `Form`, so no decode golden pins those dispatcher paths.

**Need.** A change to the scope-kind string, to the gate order, or to the record shape that carries the kind removes a typed definition with the suite green. We need one synthesized fixture per remaining promoted kind, with a decode golden that pins the typed definition it produces.

### EV-02. Generate goldens that do not separate their inputs

**Question.** Which generate-writer behaviour do the `attributes`, `sketch_link`, and `topology_base` goldens each pin?

**Known.** `crates/cadmpeg-codec-f3d/tests/golden/generate` holds `attributes.bin`, `sketch_link.bin`, and `topology_base.bin`. The three are byte-identical and each is 2,055 bytes. Their source fixtures under `tests/golden/fixtures` differ from one another, and their decode goldens under `tests/golden/decode` differ from one another. The generate writer therefore maps three different inputs to one output, and two of the three names separate nothing.

**Note.** The three goldens are the only pins on the generate lane's Design BulkStream output. That output holds the fabricated members DR-57 names — the literal `Body`, the all-zero envelope GUID, and the ordinal-derived browser-node GUID — and the body-key assignment that DR-57 and MA-13 name. No golden separates any of them, so a change to those members moves one 2,055-byte file or none.

**Need.** A reader of the golden tree counts three generate cases where one exists. We must find whether the generate lane is meant to discard what separates these inputs. If it is, two names must go or must state that they are duplicates. If it is not, the writer drops content that the decode side keeps.
