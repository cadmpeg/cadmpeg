# Autodesk Fusion 360 `.f3d`: Open Items

This document lists the parts of the F3D format that we do not know. The specification `f3d.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

A reference to the specification gives the section number and the start of the paragraph. An example is `f3d.md` §3.1 "**The ACT segment.**". Do not use line numbers. Line numbers become incorrect when the specification changes. The `scripts/check-doc-anchors.py` command makes sure that each reference finds exactly one paragraph.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Container, header, and design records

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

### DR-13. Other `WorkPoint` `refType` values

**Question.** What construction does each `refType` value other than `5`, `7`, `8`, `10`, `14`, and `20` select?

**Known.** `f3d.md` §3.1 "A direct `WorkPoint` scope" gives the member order, the version gates, the counted input-reference run, and the construction rules, input arities, and carrier envelopes for values `5`, `7`, `8`, `10`, `14`, and `20`. Value `20` uses the finite evaluated `PathDistance` scalar as a normalized path fraction. The stored `point3d` is the solved position for every value, so a reader needs no join to place the point. The decoder retains every other `refType`, the serialized input count, and every input reference without assigning a rule-specific meaning.

**Need.** A writer must emit the `refType` that matches every other input form it writes, and a neutral model that edits such an input must know which rule re-solves the point.

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

### DR-22. Form-33 `Combine` body recipe

**Question.** Which field selects the input body for a form-33 `Combine` body-recipe identity that intersects more than one input body and has no matching occurrence elsewhere in the Design stream?

**Known.** `f3d.md` §3.1 "A `Combine` scope stores" gives the complete persistent identity and the agreement rule. The identity is the stream GUID, the asset GUID, the context GUID, the ordered `(Design reference, form)` clauses, the recipe Design id, and the recipe selector. Repeated occurrences of that identity select the same stable history body when every resolved occurrence agrees.

**Need.** This item is the case in which the agreement rule has no input. The neutral model then has no body selection.

### DR-24. Class-365 whole-body operand fields

**Question.** What do the class-365 whole-body operand fields after the asset UUID and the context UUID hold? This question excludes the bounded nested-record join and the body-recipe join.

**Known.** `f3d.md` §3.1 "A class-365 whole-body member" gives the reference count, the ordered `(Design reference, form)` pairs, the asset UUID, the context UUID, the `u32 2`, the four zero bytes, the paired header, and the nested indexed headers. The `u32 2` is a literal that never varies. The four bytes after it are not always zero, and the u32 after those takes a broad range of small values. The `0x01`-tagged value before the two UUIDs is a reference whose target is the containing entity plus three, so the two zero bytes after it are that reference's flags.

**Need.** We must know what the two variable u32 select to write a complete class-365 member.

### DR-25. Base Feature six-byte fields

**Question.** What do the six-byte fields after the Base Feature body suffixes and after the Base Feature record references hold?

**Known.** `f3d.md` §3.1 "**References.**" resolves the six bytes that follow an eleven-byte element: they are the target entity ID's high half and the two reference flags. A fifteen-byte element already carries the full entity ID, so its six bytes are instead the two flags and a further u32 member of the element. The `u16 0` then `u32 1` form is that member holding `1`: the flags are both clear, and a cross-segment reference would put `1` in the second flag byte rather than in the u32. The value is uniform across a scope's body-entity run.

The class-409 primary paired with class-262 uses the same six-byte fields on its body, repeated passive-reference, and result-record entries as the ordinary Base Feature form. Its non-empty frame length is `262 + 52N`; its non-empty shared metadata field is two bytes, and its zero-body frame is 258 bytes with a six-byte shared metadata field. The class-444 primary paired with class-263 uses the same six-byte fields on its body, repeated passive-reference, and result-record entries; its passive count field has repeat marker `0`, its non-empty shared metadata field is two bytes, and its zero-body form has fourteen zero bytes after the shared metadata record. This fixes the class-keyed envelopes but does not assign a meaning to the six-byte fields.

The class-377 primary paired with class-259 is a separate direct body-reference envelope. Its body entity suffix is a u32 at primary-relative offset 40 with a ten-byte zero tail; it does not use the six-byte result-body element runs.

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
- the targets of the u32-counted reference run of the rectangular-pattern class
- the u64 key and the u64 values of the pattern-table map, and the u32 of the pattern-table run
- the first of the two text-frame references
- the zero `u8` that closes the circular-pattern class

**Known.** `f3d.md` §3.1 "A sketch-relation class writes" gives the member sequence of each class, and each sequence closes the record on its exact end. The rectangular-pattern run count has settled meaning: zero selects a seed-to-final span and a nonzero count selects adjacent spacing. The decoder retains the exact count. The targets in that run and the other listed members have no assigned meaning. The decoder consumes them and transfers no unresolved value from them.

**Need.** A pattern relation must be written back from a neutral model, and these members must carry the values the source would have written. The map keys and values are small integers in the range of record indices, so they may be a per-instance record grouping that a writer must rebuild rather than copy.

**Note.** Retain circular-pattern scope record indices `2918`, `3342`, `3752`, `4268`, and `16314` as anchors for the unresolved relation members. These indices do not settle the relation fields or operation semantics.

### DR-29. `EntityGenesis` value `0x4`

**Question.** What does the `EntityGenesis` value `0x4` mean?

**Known.** `f3d.md` §3.1 "A sketch-curve record contains" gives the origin bitfield and assigns bits `0x2`, `0x8`, `0x80`, `0x100`, and `0x200`. Bit `0x8` marks source points and curves imported from DXF; a generated circular-arc center point carries zero. Bit `0x200` marks a result curve generated by a sketch offset; the operation's source curve keeps its independent origin value. A sketch-text record can carry `0x4` while its frame curves carry `0x0`. Another authored sketch-text record carries `0x0`, so `0x4` does not identify every sketch-text entity.

**Need.** A writer must emit the value the source would have written. Without the bit meaning, a neutral model cannot choose between `0x0` and `0x4` for a sketch-text entity.

### DR-30. `sketch_attrib_def` member `ref_b`

**Question.** What does `ref_b`, the second member of the `sketch_attrib_def` payload, name?

**Known.** `asm.md` §5.6 "`sketch_attrib_def` is source-link metadata" names it, gives its position in all three payload forms, and gives its range. It is zero in most links, and the decoder keeps a non-zero value as written. A non-zero value repeats across the links of one stream and is drawn from a small set per document. Where a document has sketch text, the values that recur most are the Design entity ids of its sketch-text records, and the `sketch_curve_id` beside them matches no sketch-curve record's persistent identity in that document. Where a document has no sketch text, non-zero values still occur and the `sketch_curve_id` beside them does match a stored sketch-curve identity. So the field names neither a sketch text in particular nor, by itself, the namespace `sketch_curve_id` is drawn from. One document spells it `18446744073709551615` throughout, the all-ones 64-bit pattern, where `0` is the value every other document writes for a link with no such reference.

**Need.** A writer must choose the value to emit for a link a neutral model does not carry. Without the meaning, only a link decoded from source restores it.

### DR-31. Other `Pipe` section and hollow forms

**Question.** Which primary-header values select square, triangular, and hollow `Pipe` sections? How is section size measured for the noncircular shapes, and where is hollow-wall placement encoded?

**Known.** Primary-header offset 29 value `1` selects a circular section and offset 30 value `1` selects a filled section. For that form, scalar ordinal two is the outside diameter and scalar ordinal three is an inactive positive thickness. The section is one filled disk and has no inner boundary. The settings reference contains a u32 and one finite double.

**Need.** A writer needs the selector values and dimension conventions for every supported generated section. Hollow forms also need the direction in which thickness changes the section boundary. **Settling specimens:** otherwise equal pipes with each section shape and with the hollow option both off and on.

### DR-32A. Component records that share one component GUID

**Question.** May two local component-occurrence carriers in one Design stream carry equal component GUIDs and unequal component-record references?

**Known.** `f3d.md` §3.1 "A local component occurrence is an indexed carrier" states that equal component GUIDs name the same reusable local component definition, and each carrier stores the same u64 component-record reference twice. The validator holds one component-record reference per component GUID per stream and reports a carrier that contradicts an earlier one.

Documents exist with two unplaced ordinal-one carriers whose component GUIDs are equal, whose occurrence GUIDs differ, and whose component-record references differ. Both carriers satisfy every fixed member of the 229-byte frame, and both duplicate their own component-record reference across offsets 24 and 197, so neither is a misread frame.

**Need.** The reading decides whether the component GUID or the component-record reference is the component definition's identity. If several records may describe one definition, the validator claim is too strong and the neutral component identity must come from the GUID alone. If not, one of the two carriers belongs to a second definition and the GUID is not an identity. Nothing yet separates the two readings, so the validator keeps the stronger claim and reports the second carrier.

### DR-61. u64 values in a cross-document `Combine` selector tail

**Question.** What do the two u64 values around the fixed `u32 48` in a cross-document `Combine` body-selector tail mean?

**Known.** `f3d.md` §3.1 "A tool body-selection record" gives the complete selector grammar and the independent occurrence, external-body, segment, asset, link, property, and version fields. The first u64 follows u32 `9` and u16 `2`. The second follows u32 `48`. The two values are retained in source order and can differ between selectors that share the same owning scope.

**Need.** Their meanings determine whether they participate in persistent body identity and how a writer derives them from a cross-document body selection.

### DR-62. Point selectors and flags

**Question.** What semantic point roles do selector values `0`, `1`, `2`, and `4` identify? What does state value zero or one select? What do the one, seven, or eight versioned flag bytes before the point coordinates select?

**Known.** `f3d.md` §3.1 "A sketch-point geometry payload" gives the complete class-version-0, class-version-8, class-version-10, and class-version-11 member sequences. `f3d.md` §3.1 "`paired_reference` resolves" gives both companion prefixes, both reference encodings, the complete incident-curve run, and the inverse point link. `f3d.md` §3.1 "When the final eleven bytes" gives all three sketch-ownership joins and defines a record with no join as unowned Geometry.

Selector-state pair `(1,0)` is used by NURBS incidence and current-line auxiliary or control geometry. Pair `(4,0)` occurs only on points incident to line type `AE42BAB6-643F-4169-A33C-529C8E0A4D84`. Pair `(2,1)` has no incident curve.

**Need.** The mappings determine which point and curve records are construction or helper geometry, which records can participate in a neutral profile, and which versioned flags and selector-state pair a writer derives from each neutral point role.

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

### MA-05. Canvas crop

**Question.** Which Canvas fields hold the crop state?

**Known.** `f3d.md` §3.1 "A `Canvas` scope names" gives the Canvas geometry record. Its geometry-prologue byte 14 is the visibility Boolean. The order of the boundary-segment endpoints gives the independent u and v mirroring states. The record also holds the opacity, the plane frame, the label, and the image asset. It names no crop field. The decoder transfers visibility, mirroring, opacity, and the plane frame.

**Need.** A neutral canvas needs the crop state to show the image correctly.

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

## 5. Test evidence

### EV-01. Typed feature projection reached only by a direct call

**Question.** Which scope kinds does the feature dispatcher promote to a typed definition when a real document supplies them?

**Known.** `crates/cadmpeg-codec-f3d/src/design/feature_project.rs` holds thirteen gates that promote a scope kind to a typed definition. Twelve are arms of one chain in `project_parameter_design_with_edge_identities` that tests `scope.kind` and falls through to `FeatureDefinition::Native`. The thirteenth is in `bind_form_cages`, which filters the scope list for kind `Form`. A kill test disabled each gate in turn, so that the scope fell through to a native record, and ran the complete f3d suite. Seven gates stayed green with the gate disabled: `BaseFlange`, `RemoveBody`, `SurfaceStitch`, `CopyPaste`, `CopyPasteBodies`, `Base Feature`, and `Form`. A synthesized dispatcher test now exercises the first six of these gates with their typed operation records. A synthesized archive-scan test exercises the `Form` cage-binding gate. The dispatcher also has synthesized end-to-end tests for `JointOrigin`, `WorkPlane`, and `WorkPoint`; `WorkAxis` was already covered. The pair `SplitFace` and `DeleteFace` also has dispatcher coverage.

The projector leaves are tested. `crates/cadmpeg-codec-f3d/src/design/tests.rs` calls `project_remove_body` and `project_surface_stitch` and their siblings directly, with a scope value the test builds. The synthesized dispatcher test also checks the six typed definitions returned by the complete projector. `crates/cadmpeg-codec-f3d/src/tests.rs` supplies a minimal Design stream to the `Form` archive-scan gate and checks the resulting typed definition. No golden fixture under `crates/cadmpeg-codec-f3d/tests/golden/fixtures` carries `BaseFlange`, `RemoveBody`, `SurfaceStitch`, `CopyPaste`, `CopyPasteBodies`, `Base Feature`, or `Form`, so no decode golden pins those dispatcher paths.

**Need.** A change to the scope-kind string, to the gate order, or to the record shape that carries the kind removes a typed definition with the suite green. We need one synthesized fixture per remaining promoted kind, with a decode golden that pins the typed definition it produces.
