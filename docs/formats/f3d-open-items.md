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

### DR-09A. Sheet-metal settings compatibility and override encoding

**Question.** What is the Hem Tangent bend-position code, what do the compatibility values `1` and `4` encode, what does the version-3 rule-reference suffix encode, and how do the settings mask bits, optional slots, and override bundle tails map to independent rule controls?

**Known.** `f3d.md` §3.1 "The common Hem controls start" defines Adjacent code `3`, Flip `0`/`1`, the two null legacy references, and the form-specific owner layouts. "The common sheet-metal settings carrier" defines the versioned optional-reference runs, typed rule and presentation links, feature owner, paired header, and context. Mask `4` with slot 1 reaches BendRadius; mask `240` with slot 2 reaches BendWidth, BendDepth, and BendRemnant. The version-3 rule-reference suffix is `00 00` or `01 00 00`; its byte form remains native. Those joins do not assign each mask bit a meaning or define the suffix control, remaining slots, and bundle tails.

**Need.** A controlled Tangent Hem and one-control-at-a-time rule overrides must separate the bend-position code, compatibility lanes, individual mask bits, and optional slots. Paired controls must distinguish the two version-3 rule-reference suffix forms. A complete bundle grammar must account for each byte after the verified owner prefixes before a writer generates those bundles.

### DR-12. Placement `refType` values

**Question.** What construction does each `refType` value of the placement class select, and how does a nonzero `extraOffset` alter that construction?

**Known.** `f3d.md` §3.1 "A `Sketch` scope joined" gives the member order, the version gates, the identity-marked matrix, and the unmarked version-4 matrix. The independent `extraOffset` reference reaches a scalar owner linked to an `ExtraOffset` length parameter. `refType` is a u32 whose value does not change the member sequence. The scalar-role table defines the ordered `values` names and direct-reference shapes. Owner-scope references remain independent of the direct geometry slots, and the solved matrix does not select the construction rule. Code `15` has a `RotateAngle` scalar and two face-selection lineages; those operands do not distinguish the exact plane-construction subtype.

**Need.** We must know which references a given `refType` requires to write a placement that Fusion re-solves rather than accepts as stored. A controlled nonzero `ExtraOffset` must establish its direction and effect for each supported construction.

### DR-14. `SurfacePatch` boundary-side ordinal mapping

**Question.** Which `PatchFlip` ordinal selects each continuity-direction Boolean and incident face?

**Known.** `f3d.md` §3.1 assigns `PatchFlip` to the per-boundary u32 at `R + 26` and defines `PatchScale` as the continuity weight. Raw side ordinals `0` and `2` remain distinct. The version-7 scope's whole-surface normal Flip is independent of this ordinal.

**Need.** One supported edge with two distinct incident faces must be authored with each continuity-direction Boolean while continuity, edge order, and weight remain fixed. The saved ordinals and the API's first co-edge face must identify the side mapping.

### DR-19. Construction-group fields

**Question.** What do the construction-group scalar fields hold? What does the ordinary variant byte control? What allocates the ordinal? What does compact relative-placement `false` mean, and what distinguishes the two dual-transform slots?

**Known.** `f3d.md` §3.1 defines the feature-specific roles, the Sketch retained-input role `0x81 << 32`, and the pattern path-carried seed role `0x100 << 32`. The group holds a nonzero u32 `ordinal`, a nonnegative finite f64 `scalar`, and a second copy of `ordinal` that one container generation omits. The ordinary `variant` is zero or one; the class-gated legacy body-tail pairs have separate grammars. `ordinal` does not encode the timeline feature ordinal, and Sketch groups can have different ordinals in one scope. Its allocation event and the unit of `scalar` remain unsettled. The two auxiliary references retain ordered paths. A single matrix stores their relative placement when both rigid path placements are defined; compact `true` represents identity placement. A missing path pair supplies no placement equation. A dual carrier retains two matrices in order. The full-round Fillet form supplies its center-face selection independently of the relative-placement carrier.

**Need.** Controlled evaluation and save pairs must distinguish the scalar unit, ordinal allocation event, and ordinary variant meaning. A compact-`false` control must separate invalid, default, identity, and inactive interpretations. Independent occurrence and local-geometry edits must distinguish the dual slots. Auxiliary-path forms without an admitted rigid placement need complete typed grammar before they define a relative-placement equation. Preserve these fields and forms until those distinctions are established.

### DR-25. Base Feature six-byte fields

**Question.** What do the six-byte fields after the Base Feature body suffixes and after the Base Feature record references hold?

**Known.** `f3d.md` §3.1 "**References.**" resolves the six bytes that follow an eleven-byte element: they are the target entity ID's high half and the two reference flags. A fifteen-byte element already carries the full entity ID, so its six bytes are instead the two flags and a further u32 member of the element. The `u16 0` then `u32 1` form is that member holding `1`: the flags are both clear, and a cross-segment reference would put `1` in the second flag byte rather than in the u32. The value is uniform across a scope's body-entity run.

The class-360 primary paired with class-258 and the class-409 primary paired with class-262 use the same six-byte fields on their body, repeated passive-reference, and result-record entries as the ordinary Base Feature form. Their non-empty frame length is `262 + 52N`; their non-empty shared metadata field is two bytes, and the class-409 zero-body frame is 258 bytes with a six-byte shared metadata field. The class-444 primary paired with class-263 uses the same six-byte fields on its body, repeated passive-reference, and result-record entries; its passive count field has repeat marker `0`, its non-empty shared metadata field is two bytes, and its zero-body form has fourteen zero bytes after the shared metadata record. This fixes the class-keyed envelopes but does not assign a meaning to the six-byte fields.

The class-377 primary paired with class-259 is a separate direct body-reference envelope. Its body entity suffix is a u32 at primary-relative offset 40 with a ten-byte zero tail; it does not use the six-byte result-body element runs.

The class-290 primary paired with class-261 uses the same six-byte fields on its body, repeated passive-reference, and result-record entries. Its non-empty frame length is `261 + 52N`, its count field has six zero bytes before `u32 N`, and its shared metadata field is two bytes.

**Need.** We must know what the per-element u32 selects to write a Base Feature from a neutral model. It is `1` on one scope and `0` on every other, and it does not track the body count, so we cannot say what selects it.

### DR-26. Sketch-text fields

**Question.** What do the residual `txt_tag` style bits and four-byte style payload, its second post-height Boolean, and the `textex_tag` orientation and auxiliary flags select?

**Known.** `f3d.md` §3.1 "In a `txt_tag` record the twenty-nine bytes" defines the bold bit, horizontal-flip Boolean, anchor coordinates, and versioned pre-string lane. "The `txt_tag` leading block names four frame lines" defines the leading and post-string references. "A `textex_tag` record's class tail opens" defines the cached natural width and its independence from frame width. "The legacy text presentation suffix" defines the default presentation payload and separates it from glyph colour. The first of the three bytes after horizontal alignment and the byte after vertical alignment can both be set. That paired state does not distinguish an axis-specific flip from an echoed flag. Alignment enums vary independently. The second `txt_tag` Boolean can be set with the first clear; its glyph-axis meaning remains unresolved. The numeric font weight does not assign a meaning to an auxiliary flag.

**Need.** Independently authored horizontal-only and vertical-only flips must distinguish the legacy orientation flags. An authoring control that changes the serialized style payload must identify each residual style bit or field. API text-style enum values alone do not assign serialized italic or underline bits.

### DR-27. Remaining sketch-pattern state

**Question.** What do the third rectangular-pattern flag and the positive circular closing flag select, and how are rectangular map keys assigned with two active directions?

**Known.** `f3d.md` §3.1 "The tangency subclass flags refer" defines the bounded endpoint choices and tangent-direction sign in semantic return order. "The rectangular-pattern reference run" defines the optional line-direction slots, the explicit extent flag, and direction-one symmetry. The reference count does not choose distance form. "The pattern map is an optional instance-identity cache" defines persistent curve identities, repeated groups, stale entries, circular keys, and one-row triangular rectangular keys. "The u32 run after the pattern map" defines suppression ordinals. "The text-frame class's first reference" defines its optional center point. The circular closing byte is Boolean; zero accompanies the one-direction partial-angle distribution. The format model assigns no positive-state meaning from public API property order alone.

**Need.** A positive direction-two symmetry control must bind the third rectangular flag to its serialized state. A circular pattern with a positive closing flag and a partial angular extent must distinguish symmetric from one-direction distribution. A non-empty two-direction rectangular map must identify its general instance-to-key rule. Empty maps remain valid and do not require that rule.

### DR-31. Editable Pipe section-type carrier

**Question.** Which native carrier stores the editable `Pipe` section type?

**Known.** `f3d.md` defines the compact and legacy operation prefixes, the independent `isHollow` Boolean, the four scalar roles, and the outside-size and inward-thickness conventions. The two bytes after the operation are opaque. Square hollow, circular hollow, and triangular solid sections can share that pair. A section shape can be bound from unique saved topology.

**Need.** Independently authored section-type changes must hold the path, fractions, hollow state, size, and thickness fixed and identify the changing semantic carrier through its type registration and owner links.

### DR-61. u64 values in a cross-document `Combine` selector tail

**Question.** What do the two u64 values around the fixed `u32 48` in a cross-document `Combine` body-selector tail mean?

**Known.** `f3d.md` §3.1 "A tool body-selection record" gives the complete selector grammar and the independent occurrence, external-body, segment, asset, link, property, and version fields. The first u64 follows u32 `9` and u16 `2`. The second follows u32 `48`. The two values are retained in source order and can differ between selectors that share the same owning scope. The exact class-`329` forms already admitted by the decoder show the first value repeated by selectors in one operation and zero in the observed operation without the optional property/version tail. The second value differs between selectors and reads as a finite little-endian f64 in the four observed selectors. The first value's byte sequence also occurs in other fixed reference records, so it is not a unique external-body identity token.

**Need.** Their meanings determine whether they participate in persistent body identity and how a writer derives them from a cross-document body selection.

### DR-62. Point selectors and flags

**Question.** What semantic point roles do selector values `0`, `1`, `2`, and `4` identify? What does state value zero or one select? What do the one, seven, or eight versioned flag bytes before the point coordinates select?

**Known.** `f3d.md` §3.1 "A sketch-point geometry payload" gives the complete class-version-0, class-version-8, class-version-10, and class-version-11 member sequences. `f3d.md` §3.1 "`paired_reference` resolves" gives both companion prefixes, both reference encodings, the complete incident-curve run, and the inverse point link. `f3d.md` §3.1 "When the final eleven bytes" gives all three sketch-ownership joins and defines a record with no join as unowned Geometry.

Selector-state pair `(1,0)` is used by NURBS incidence and current-line auxiliary or control geometry. Pair `(4,0)` occurs only on points incident to line type `AE42BAB6-643F-4169-A33C-529C8E0A4D84`. Pair `(2,1)` has no incident curve.

The selector-state pair and the versioned flags do not select the neutral construction Boolean. The neutral projection keeps points non-construction, and a valid `TextFrame` relation selects construction curves independently of their endpoint point records. Pair `(4,0)` can occur on a bounded line used by a midpoint relation and on a bounded line with no sketch relation; neither form is classified as construction from the pair alone. Profile admission remains determined by the resolved curve graph and the explicit `TextFrame` exclusion.

Pair `(2,1)` is the standalone-point form. Its companion has an incident-curve count of zero, and the point remains non-construction.

**Need.** The mappings determine which point and curve records are construction or helper geometry, which records can participate in a neutral profile, and which versioned flags and selector-state pair a writer derives from each neutral point role.

### DR-67. Face source-shape semantics

**Question.** Which source-shape semantics and face-maker operation do the ordered identities in a `Face` source carrier express? Which source carrier grammar does each unadmitted class pair use?

**Known.** `f3d.md` §3.1 "A `Face` scope has an ordered source-shape carrier" defines the admitted `398 / 462`, `394 / 311`, and `356 / 309` frames. Their source identities use the fixed persistent identity frame and can name different source-shape families. The `364 / 261` carrier is not an admitted source frame.

**Need.** A neutral `FaceFromShapes` projection requires a proven mapping from each ordered source identity to its source shape and face-maker semantics. Until that mapping is established, the source carrier remains native data.

### DR-68. Grouped `SurfacePatch` recipe-reference precedence

**Question.** Which field selects the edge reference for each member of a
grouped `SurfacePatch` boundary when more than one recipe reference has a
nonempty exact edge-candidate set?

**Known.** Each recipe reference retains its selector, token, Design references,
source order, and complete exact and alternate candidate sets. Source order does
not establish that an earlier reference overrides a later reference. The
decoder projects a grouped member only when all of its nonempty exact reference
sets contain the same sole edge and the complete group maps to distinct edges.
An ambiguous or contradictory exact reference retains the native group and does
not fall through to generic edge reconstruction.

**Need.** We must decode the reference role or selector relation that binds one
edge to each group member and rejects contradictory references.

### DR-69. Single-member edge-treatment identity

**Question.** Which serialized operand identity binds the sole edge member of a
`Fillet` or `Chamfer` scope when its recipe and persistent-selection fields do
not resolve the edge?

**Known.** The complete ASM history transition identifies every predecessor
edge deleted by the operation. Deletion is operation-level topology evidence;
it is not by itself an operand-to-edge identity relation. The decoder retains
an unresolved sole operand as native even when the complete transition deletes
one predecessor edge.

**Need.** We must decode the operand binding before assigning a deleted
predecessor edge as the selected treatment edge.

### DR-74. Lost-edge runs shorter than the operand list

**Question.** Which operands of a counted edge group do the records of a
shorter unresolved run name?

**Known.** `f3d.md` §3.1 "An unresolved edge-selection record consists of"
gives the record layout, the run chaining through following headers, and the
group that a run terminates at. A run with the group's operand count carries
one record per operand. A ten-operand group with a one-record run occurs.

**Need.** A decoder must assign each unresolved record to its operand before
it can bind a shorter run. The decoder binds only a run whose cardinality
equals the operand count. The records of any other run stay unrepaired lost
edge references.

### DR-75. Local occurrence carrier GUID

**Question.** What does the LP-UTF16 carrier GUID at offset 44 of a local
component-occurrence carrier identify?

**Known.** `f3d.md` §3.1 "The component-record reference names the component
definition record." gives the component-definition join through the
component-record reference and the naming-space binding. Two carriers that
name one component definition record can store different carrier GUIDs. The
`CopyPaste` and `R-Pattern` joins select carriers by equal carrier GUID.

**Need.** A writer must emit this GUID for each new occurrence.

**Conflict.** When a component definition record stores no naming-space
binding, the decoder keys the neutral component definition by the carrier GUID.
Two carriers of one definition then project as two definitions.

### DR-76. Nested local occurrence placement

**Question.** How does a local occurrence whose owner entity is the root entity
of a non-root component compose into the document frame?

**Known.** `f3d.md` §3.1 "The component-record reference names the component
definition record." gives the owner entity and the root-instance record. An
occurrence that the root component contains is a child of the root occurrence,
and its transform maps the component definition into the root component frame.

**Need.** A reader must place every occurrence of a nested component in each
occurrence of its containing component. The decoder does not place a nested
occurrence and reports `assembly.local-occurrence-unplaced`.

### DR-77. Operation placements of identity-frame carriers

**Question.** Does the `CopyPaste` source placement or the `R-Pattern` first
instance transform equal the placement of the carrier it names?

**Known.** `f3d.md` §3.1 "A local component occurrence is an
occurrence-placement record" gives the identity frame and its identity
placement. `f3d.md` §3.1 "Legacy `CopyPaste` is a local component-occurrence
operation." and `f3d.md` §3.1 "When a one-direction `R-Pattern` repeats a local
component occurrence" give the scope transforms for the source and the seed.

**Need.** A reader must select one placement for each occurrence.

**Conflict.** When the source or seed carrier is an identity frame and the scope
transform is not the identity, the two placements disagree. The decoder uses
the scope transform.

### DR-78. Symmetric Extrude distance measurement

**Question.** Which field states whether the distance of a symmetric
distance `Extrude` is the travel on each side or the whole travel?

**Known.** `f3d.md` §3.1 "An Extrude scope can carry one or both of its
distance and taper inputs" gives the fixed lanes. One symmetric `Extrude`
without indexed parameters stores lane zero as `1.2` cm on a Sketch in the
plane `x = 150.5` mm. Its operation history holds planes at `x = 138.5` mm
and `x = 162.5` mm, which is `12` mm on each side. The prologue stores
travel direction `3`, face-extend option `2`, and side extents `[1, 0]`.

**Need.** The neutral symmetric extent states the whole travel. A reader must
know the measurement form to convert the stored distance. The decoder keeps
the native feature when a fixed lane carries a symmetric distance.

**Conflict.** The decoder converts an `AlongDistance` parameter of a symmetric
`Extrude` to the whole travel without reading a measurement form.

### DR-79. Typed-reference Design record grammars

**Question.** What are the sketch, sketch-relation, construction-recipe, parameter-scope, and feature-scope grammars in the container generation that writes typed references?

**Known.** `f3d.md` §3.1 "The typed compact parameter form is the compact parameter form" gives the parameter form, and `f3d.md` §3.1 "In the container generation that writes typed references" gives the parameter-owner frames. In that generation every local reference carries its LP-ASCII 36 target type GUID, so each reference is 51 bytes instead of eleven. Sketch and feature records also store typed references. The decoder reads the typed parameter and owner forms only.

**Need.** Sketches, constraints, and feature history in documents of this generation stay native until each fixed-offset grammar reads references by their encoded width.

### DR-80. Typed parameter tail reference target

**Question.** What record does the typed parameter tail reference name?

**Known.** Its target type GUID is `B0556F58-EE09-46A7-B18F-E296F48E5B13`, and its target record index varies between documents. The twelve-byte tail of the other forms is the same reference without an inline type GUID.

**Need.** A writer must emit this reference to build a typed compact parameter.

### DR-81. Other typed compact parameter trailers

**Question.** What do the typed compact parameter frames with other expression trailers or a null owner on a feature input encode?

**Known.** `f3d.md` §3.1 "The typed compact parameter form is the compact parameter form" gives the owned and document forms. Some typed frames store the null owner reference with a feature-input or dimension `source_kind` (for example `HoleDepth` or `TaperAngle`) and the expression trailer `00 00 00 01 00`. Some owned frames store the expression trailer `00 01 00 00 00`. The decoder reports these frames as `parameter.record-unparsed` and does not decode them.

**Need.** An ownerless feature input has no owner binding in the current parameter model, and the meaning of the set trailer bytes is unknown.

## 2. External references

### DR-82. Other joint freedom codes and the limit flags

**Question.** Which degrees of freedom do freedom codes other than `1`, `2`, and `8` name, what do the two joint-record limit flag bytes select, and in which unit is a translation limit stored?

**Known.** `f3d.md` "Assembly operands" gives the joint and motion records, the freedom entry, codes `1`, `2`, and `8`, and the limit lanes. Freedom codes `0`, `6`, and `7` also occur, alone and in pairs such as `2` with `7`, `0` with `8`, and `2` with `6` and `7`. Both flags are `1` on a joint with a minimum and a maximum.

**Need.** Cylindrical, pin-slot, planar, and ball joints combine freedoms. A joint with only one limit needs the flag that enables it.

### DR-83. Attributed occurrence members

**Question.** What do the two bytes before the occurrence ordinal and the marked record reference after the occurrence key of an attributed local component occurrence store?

**Known.** `f3d.md` §3.1 "An attributed local component occurrence" gives the frame. The two bytes are `00 00` in most frames and `01 00` in one. The reference names a different record in each frame, in increasing order.

**Need.** The two bytes can be a grounding or visibility state of the occurrence.

### XR-01. `neutronData` with a different GUID

**Question.** What does `neutronData` mean when its GUID is different from the `neutronRole` GUID?

**Known.** `f3d.md` §1.4 `RedirectionsStream.dat` states that `neutronData` is an independent property. It does not need to equal `neutronRole`. The decoder keeps the value as a note string.

**Need.** We must know the meaning to build the external reference in a neutral model.

### XR-02. Non-empty `ComponentReferenceData.json`

**Question.** Which member names occur in a non-empty `ComponentReferenceData.json` object? What does each member control?

**Known.** `f3d.md` §1.4 `ComponentReferenceData.json` states that the object is an open document. It keeps member names and values without a closed schema. The decoder checks the envelope and makes no neutral projection.

**Need.** We must know the member names and their meanings to build component references in a neutral model.

### XR-04. Occurrence-placement reference runs

**Question.** What do the counted reference run after the matrix, the modern tagged u32 run, the two modern closing references, and the legacy typed target-reference fields name?

**Known.** `f3d.md` §1.4 "**Placement.**" gives the class, the modern and legacy layouts, the instance discriminator, the identity-marked matrices, and the exact 403-byte and 531-byte legacy closures. The modern counted run reaches both local and cross-document targets. The two modern closing references name the same pair of entities for every placement of one document, so neither depends on the placement. The legacy form carries two target-reference envelopes and two opaque UTF-16 GUID fields; its role and identity-or-matrix transform are independently defined. A placement record can also carry the UTF-16 string `GatedByParent`; its position in the member sequence, its gate, and its value are not established.

**Need.** We must know the targets and field meanings to write a complete occurrence placement in each envelope. A reader takes the target path, the discriminators, the role, and the transform without resolving the remaining fields.

## 3. Material assets

### MA-04. Texture map-channel values

**Question.** What does each texture map-channel integer value mean?

**Known.** `f3d.md` §3.2 "`UnifiedBitmapSchema` and `BumpMapSchema` records" names the map-channel property. The application defines the value meanings.

**Need.** We must know the meanings to map a texture to the correct channel in a neutral model.

### MA-08. Omitted texture map-channel member

**Question.** Which member does a `TextureMap2dSchema` closure omit from its value block?

**Known.** `f3d.md` §3.2 "An `InstanceProperties` record opens with" states that such a block is one four-byte slot shorter than its closure, and that only omitting `texture_MapChannel_ID_Advanced` or `texture_MapChannel` leaves every surviving member at its schema default. The two are byte-degenerate: both are four-byte integers at adjacent positions, and the surviving pair reads `1` and `0` under either choice, which are the two members' declared defaults in either assignment.

**Need.** A writer must emit the same member set the reader expects, and the two choices shift every following member by four bytes. A texture asset authored with a map channel other than `1`, or with a non-default advanced channel id, separates them.

## 4. T-splines

### TS-05. Compact Form cage-list tail

**Question.** What do bytes 49 through 99 of the compact one-cage cage-list record hold?

**Known.** `f3d.md` §1.1.1 "A compact one-cage cage-list record is 100 bytes" gives the indexed header, the ten zero bytes, the owning Form scope record index, the one-cage count, the sole cage-object record index, the two zero bytes, and the `0x00fc` member flags. The remaining 51 bytes are retained with the native record and have no assigned semantic field.

**Need.** A writer must know the compact-form tail before it can emit this record from a neutral Form feature. The decoder can bind the sole cage from the settled prefix and retain the tail for source fidelity.

### TS-06. Radial symmetry map namespace

**Question.** Which native element domain do the six radial `105r` map records address, and how do their operands map to neutral cage elements?

**Known.** The `105r` selectors `ef`, `er`, `ff`, `fr`, `vf`, and `vr` carry even-length index runs. Their records occur with the positive `segments` count and finite `sweep` value of a `105sym 1` block. The operands do not consistently address the populated `f`, `e`, or `v` topology slots.

**Need.** A neutral radial symmetry carrier still needs a proven mapping from the native map identifiers to cage faces, edges, and vertices before a writer can generate the block from edited topology. The typed native-map carrier preserves source values but does not assert that mapping.

**Conflict.** Treating every radial operand as a local topology slot rejects valid large cages and assigns some native IDs to unrelated neutral elements.
