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

### DR-105. Hole Extents To

**Question.** Which extent controls and face operands define an Extents To hole?

**Known.** To and Through All both store a non-distance depth with the name `SavedHoleDepthInNonDistanceExtentsCase`. Control `1` with that property, no `HoleDepth`, and only start-face operands defines Through All.

**Need.** A file with a To-face hole.

**Conflict.** None.

**Note.** A Distance hole that reaches a face does not define the To-face form.

### DR-106. Hole extent leading word

**Question.** What does the u32 word before the Hole extent controls in type versions 5 and 7 encode?

**Known.** Type versions 3 and 4 do not store the word, and type version 3 stores no extent control. Every observed word is `0`, on Distance and non-distance holes.

**Need.** A Hole of type version 5 or 7 with a nonzero word, and the hole type, tap, or position setting that produces it.

**Conflict.** None.

**Note.** The decoder records the word on the native Hole extent and does not use it for projection.

### DR-09A. Sheet-metal settings compatibility and override encoding

**Question.** What is the Hem Tangent bend-position code, what do the compatibility values `1` and `4` encode, what does the version-3 rule-reference suffix encode, and how do the settings mask bits, optional slots, and override bundle tails map to independent rule controls?

**Known.** `f3d.md` §3.1 "The common Hem controls start" defines Adjacent code `3`, Flip `0`/`1`, the two null legacy references, and the form-specific owner layouts. "The common sheet-metal settings carrier" defines the versioned optional-reference runs, typed rule and presentation links, feature owner, paired header, and context. Mask `4` with slot 1 reaches BendRadius; mask `240` with slot 2 reaches BendWidth, BendDepth, and BendRemnant. The version-3 rule-reference suffix is `00 00` or `01 00 00`; its byte form remains native. Those joins do not assign each mask bit a meaning or define the suffix control, remaining slots, and bundle tails.

**Need.** A controlled Tangent Hem and one-control-at-a-time rule overrides must separate the bend-position code, compatibility lanes, individual mask bits, and optional slots. Paired controls must distinguish the two version-3 rule-reference suffix forms. A complete bundle grammar must account for each byte after the verified owner prefixes before a writer generates those bundles.

### DR-12. Placement `refType` values

**Question.** What construction does each `refType` value of the placement class select, how does a nonzero `extraOffset` alter that construction, and what does the version-16 `periodicFaceMode` select?

**Known.** `f3d.md` §3.1 "A `Sketch` scope joined" gives the member order, the version gates, the identity-marked matrix, and the unmarked version-4 matrix. The independent `extraOffset` reference reaches a scalar owner linked to an `ExtraOffset` length parameter. `refType` is a u32 whose value does not change the member sequence. The scalar-role table defines the ordered `values` names and direct-reference shapes. Owner-scope references remain independent of the direct geometry slots, and the solved matrix does not select the construction rule. Code `15` has a `RotateAngle` scalar and two face-selection lineages; those operands do not distinguish the exact plane-construction subtype.

**Need.** We must know which references a given `refType` requires to write a placement that Fusion re-solves rather than accepts as stored. A controlled nonzero `ExtraOffset` must establish its direction and effect for each supported construction. Nonzero axis-state bytes, version-15 and earlier direction controls, and version-16 periodic-face modes require a solved-frame orientation rule. An offset-plane definition also requires the scope operand role that selects its reference plane or planar face; a solved matrix and an `AlongDistance` parameter do not assign that role.

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

### DR-31. Editable Pipe section-type carrier and path parameterization

**Question.** Which native carrier stores the editable `Pipe` section type? What sets the origin and orientation of a partially consumed path?

**Known.** `f3d.md` defines the compact and legacy operation prefixes, the independent `isHollow` Boolean, the four canonical scalar owners, the settings record, the complete construction groups, the optional point-selection pair, and the outside-size and inward-thickness conventions. The two bytes after the operation are opaque. Square hollow, circular hollow, and triangular solid sections can share that pair. A section shape can be bound from unique saved topology. Forward and reverse path fractions remain exact native values. The point-selection pair does not establish a path origin.

**Need.** Independently authored section-type changes must hold the path, fractions, hollow state, size, and thickness fixed and identify the changing semantic carrier through its type registration and owner links. Partial-path reconstruction needs the native path parameter origin and orientation.

### DR-61. Cross-document selection stamp and evaluation scalar

**Question.** How does the producer allocate or derive the selection-payload stamp, and what unit and evaluation event does the scalar represent?

**Known.** `f3d.md` §3.1 "A tool body-selection record with primary index `N`" defines a u64 stamp, a u32 evaluation epoch, and a finite nonnegative f64 scalar. The scalar is not an integer identity, and the epoch is not a fixed selector constant. Selectors with the same asset, context, external body, segment, property, and version can have different stamps. Different occurrences can share a stamp. A zero stamp can occur with a valid nonzero occurrence and external-body reference, so it does not establish an absent selection or a general default. The complete native selector remains the preservation unit.

**Need.** Controlled producer saves must separate selection creation, occurrence choice, selected entity, reevaluation, source version, and relink operations. A producer trace or documented evaluator contract must establish the scalar unit and event. A stamp derivation must reproduce the known cases and predict held-out saves, or identify the allocation event and persistence boundary if the value is allocated state.

### DR-62. Producer point-property mapping

**Question.** Which producer properties or creation operations select the opaque point flags and closure state, and what is the exact producer meaning of lanes 5 and 7?

**Known.** `f3d.md` §3.1 defines the versioned point tails, companion incidence, and ownership joins. Closure `(2,1)` permits a line incidence and is not an isolated-point predicate. Lane 5 requires lane 3 and positive-zero coordinates, but zero coordinates do not imply lane 5. Lane 7 identifies a point at its single incident circular curve's center. These constraints do not establish a producer API Boolean name. Neither closure nor flags select neutral construction or replace the explicit curve graph for profile admission.

**Need.** A producer trace or independently controlled saves must isolate point creation, origin creation, constraint changes, linked and reference state, visibility, and circular-center construction. The producer property mapping must predict held-out nondefault states before a writer can derive these bytes from neutral roles.

### DR-67. Base Face material side and result choice

**Question.** Which producer fields select Side 1, Side 2, Center, New Body, and New Component for a legacy sheet-metal Base Face, and what do the opaque carrier suffix and repeated-u32/f64 lane mean?

**Known.** `f3d.md` §3.1 defines the stable Base Face scope, ordered source-profile pairs, persistent region identities, and separate sheet-metal settings. The one-identity carrier is valid; the enclosing scope is a different record. The repeated discriminator differs between Forward results and is not a constant Forward or feature-family code. The complete carrier suffix remains opaque and has not been shown to omit side or result choice. Family-2 profile clauses, nonconstant loop markers, and secondary loop programs also require preservation; a family-3-only primary-loop decoder does not cover every source recipe.

**Need.** Controlled saves must vary side and result choice independently while holding the sketch, regions, thickness, and rule fixed. Complete carrier, settings, and profile programs must be compared, with exact result topology confirming each intended change. Until the selected region, thickness, material side, and result choice are independently resolved, retain the operation as native sheet-metal Base Face data.

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

**Question.** What are the dimension-locus, construction-recipe, feature-prologue, and other fixed scope-tail grammars in the container generation that writes typed references?

**Known.** `f3d.md` §3.1 "The typed compact parameter form is the compact parameter form" gives the parameter form, and `f3d.md` §3.1 "In the container generation that writes typed references" gives the parameter-owner frames. In that generation every local reference carries its LP-ASCII 36 target type GUID, so each reference is 51 bytes instead of eleven. Sketch and feature records also store typed references. `f3d.md` §3.1 "A typed-reference parameter scope" gives the 196-byte scope tail and the typed reference table, and `f3d.md` §3.1 "A typed-reference sketch member-run record" gives the sketch container member run and the explicit head record. `f3d.md` §3.1 "A typed-reference companion prefix" gives the companion prefix, `f3d.md` §3.1 "A typed-reference Extrude scope" gives the typed and shifted typed Extrude prologues, `f3d.md` §3.1 "A typed-reference Extrude selection group" gives the selection group, and `f3d.md` §3.1 "The class-409 zero-body form has" gives the zero-body Base Feature. Typed dimension locus frames, construction recipes, and the other feature-specific fixed scope tails of this generation have no specified grammar.

**Need.** Sketch constraints and complete feature definitions in documents of this generation stay native until each fixed-offset grammar reads references by their encoded width.

### DR-89. Typed profile-identity entry fields and other profile forms

**Question.** What do the twelve bytes after the secondary identity in a typed profile-identity entry mean, what separates the two loop lists, and which form do the typed Extrude profile groups without a typed profile selection use?

**Known.** `f3d.md` §3.1 "A typed-reference profile selection is a construction-operand group member" gives the record grammar and the exact join. The twelve bytes are three u32 values; observed values are small, and the first is `0` or `1`. Some typed Extrude profile groups name members that are not typed profile selections.

**Need.** The loop roles and entry fields can distinguish outer and inner boundaries and curve orientation, and the other member form keeps those Extrude profiles native.

### DR-80. Typed parameter tail reference target

**Question.** What record does the typed parameter tail reference name?

**Known.** Its target type GUID is `B0556F58-EE09-46A7-B18F-E296F48E5B13`, and its target record index varies between documents. The twelve-byte tail of the other forms is the same reference without an inline type GUID.

**Need.** A writer must emit this reference to build a typed compact parameter.

### DR-81. Other typed compact parameter trailers

**Question.** What do the typed compact parameter frames with other expression trailers or a null owner on a feature input encode?

**Known.** `f3d.md` §3.1 "The typed compact parameter form is the compact parameter form" gives the owned and document forms. Some typed frames store the null owner reference with a feature-input or dimension `source_kind` (for example `HoleDepth` or `TaperAngle`) and the expression trailer `00 00 00 01 00`. Some owned frames store the expression trailer `00 01 00 00 00`. The decoder reports these frames as `parameter.record-unparsed` and does not decode them.

**Need.** An ownerless feature input has no owner binding in the current parameter model, and the meaning of the set trailer bytes is unknown.

### DR-85. Flagged empty mesh-collection word

**Question.** What do the flag byte at offset 20 and the u32 at offset 25 of a flagged empty mesh collection store, and can a flagged collection have bodies?

**Known.** `f3d.md` §3.1 "The mesh-collection class" gives the flagged layout. Every flagged collection is empty, and its u32 is `1`.

**Need.** The value can count mesh bodies removed from the feature. A design in which mesh bodies are inserted and removed one at a time, saved after each step, fixes the value and shows whether a flagged collection keeps bodies.

### DR-115. Mesh container decode-failure taxonomy

**Question.** Which malformed or unsupported container states does `mesh.container-undecoded` cover, and does the retained error identify the failing stage?

**Known.** `f3d.md` §1.1.2 "When a document's mesh-body metadata" defines the omission: a mesh body whose metadata or referenced `.paramesh` container fails the container grammar transfers no tessellation and reports `mesh.container-undecoded` with the entry name and the decode error. B-rep evaluation then proceeds. The specification gives the container layout; the per-stage failure taxonomy awaits definition.

**Need.** Containers that fail at each stage (magic, version, reserved run, protobuf framing, name-table order, descriptor grammar, stream encoding, declared counts) must show which failures this loss covers and whether the retained error text identifies the failing stage, before a reader can distinguish malformed input from an unsupported container generation.

**Note.** A resource-limit failure returns an error through the error path.

### DR-86. Linked component-binding entity

**Question.** What is the entity that a linked component context binding names between the component reference and the context UUID?

**Known.** `f3d.md` §3.1 "A Design component naming-space type" gives the linked form. In a Design stream that uses the linked form, every component except the root component binds in this form, and several components can name the same entity. In one such stream the entity is registered to type GUID `8ACBA2E5-5A4C-4438-8351-276DE96E6982`, add-in name `Fusion`, base-type GUID `046D6232-EDAB-414D-9CB0-A2E3F115AE1C`, and type version `4`. In others, the components name one entity that the MetaStream registers to no type and whose record it does not index.

**Need.** A writer must emit the entity to produce the linked form. The shared entity can group components by their parent component or source document.

### DR-87. Parameter-owner flag byte

**Question.** What does the flag byte after the `variant` byte of a parameter owner store?

**Known.** `f3d.md` §3.1 "A dimension or feature-input parameter without the null owner stores `owner_record_index`" gives the byte. It is zero in almost every owner. It is one only in scalar owners whose `variant` is one, and one design stores variant one with flag zero.

**Need.** The flag can mark a driven or suppressed parameter. A design that toggles one dimension between driving and driven, saved in each state, fixes the meaning.

### DR-88. Secondary asset-folder run

**Question.** What separates the assets of the secondary asset-folder run of the top-level manifest from the assets of the first run?

**Known.** `f3d.md` §1.3 "The asset-folder tail is the unique suffix" gives both runs. The secondary run is empty in most documents. In documents that also store a toolpath, animation, or simulation asset, it lists the `Simulation Model` folder, and the first run lists the Design, toolpath, and animation folders.

**Need.** A writer must place each asset in the correct run. A document with two simulation studies, or with a simulation asset and no toolpath, shows which asset kinds the secondary run holds.

### DR-91. Opposed joint frame mate

**Question.** For a joint whose frame-sense byte is `1` (opposed), how do the two operand frames' in-plane axes relate, and how does the alignment angle apply?

**Known.** In every observed opposed joint the two frames' z axes are antiparallel (`z₁ = −z₀`). For an aligned joint, the first operand's model frame is the second's turned by the alignment angle about its z axis. No observed opposed joint has a nonzero alignment angle or offset, so the in-plane axis that flips and the sign of the angle are not fixed by any sample.

**Need.** An opposed joint with a nonzero angle and asymmetric operands, whose stored placement shows the result, would fix which in-plane axis reverses (x or y) and whether the angle turns about `z₀` or `z₁`.

**Conflict.** Assuming a 180° turn about either in-plane axis, or applying the angle with the aligned sign, states a relation no sample shows.

### DR-92. Current revision of a component definition

**Question.** When a component definition is stored as revisions, each binding its own context UUID, which revision is current?

**Known.** Two real assemblies store a component definition as two record pairs that repeat one record index. Both pairs carry the component's name and each binds its own context UUID, and neither UUID occurs elsewhere in the design. The later pair is longer and carries typed references.

**Need.** A design whose persistent identities use one of the two UUIDs, or whose component properties differ between the revisions, must show which revision Fusion treats as current.

**Conflict.** Treating either revision as the only binding discards a naming space a persistent identity could select.

### DR-93. Sheet-metal rule word and entry run

**Question.** What does the u32 word at rule-record offset 21 state, and how long is the rule's entry run?

**Known.** In two CC0 sheet-metal parts the word is 3. It is followed by four LP-UTF16 expression and f64 value entries (`Thickness`, `1`, `Thickness * 0.5`, `Thickness * 2.0`), then a zero u32 and the `Thickness` entry again. So the word is not the entry count.

**Need.** Rules with a different number of entries, or a controlled edit of one rule value, would fix the word's meaning and the bounds of the entry run.

**Conflict.** Treating the word as an entry count reads one entry fewer than the record holds.

### DR-94. Freedom entry word and trailing value

**Question.** What does the u32 word before a freedom code state, and what does the f64 after the code of a word-`1` entry state?

**Known.** Word-`3` entries occur in current-document joints. Word-`1` entries occur in the motion records of `As-built` joints in inserted designs. The trailing f64 of those entries is `0` or 2π. Their freedom codes select the same motion as word-`3` codes: each word-`1` code-`2` joint's two connector frames share an origin and a z axis.

**Need.** Joints that differ only in their motion settings, saved once with each word, would fix what the word selects. Varying such a joint's angle would show whether the trailing value tracks it.

**Conflict.** Treating the trailing value as an angle, a period, or a limit states a meaning no sample fixes.

### DR-95. Rectangular-pattern direction selection kind `16` and the reference at offset 138

**Question.** What does selection kind `16` select in a rectangular-pattern direction record, what do its origin and displacement mean, and what record does the marked reference at offset 138 name?

**Known.** `f3d.md` §3.1 "A rectangular-pattern direction record is a 220-byte frame" gives the frame, the U-then-V order, and the direction as the normalized displacement. Kind `8` selects a linear edge, with the edge start point and the edge vector. A kind-`16` record stores a unit displacement and an origin on the selected entity; the specification does not name the entity kind. The reference at offset 138 names a record outside the scope reference table.

**Need.** A writer must emit the selection kind, the origin, and the reference at offset 138 for a direction that does not come from a linear edge, and a reader must know whether a kind-`16` origin is a point of a planar face, an axis, or another entity.

### DR-97. Freedom entry second value `0`

**Question.** What does a freedom entry state when its second f64 is `0` rather than π?

**Known.** `f3d.md` "Assembly operands" gives the freedom entry and its second f64. The one zero-lane entry seen is a word-`1` rotation entry whose joint record enables both limit flags, with a −58° minimum and a 0° maximum. Word-`1` rotation entries of unlimited joints and word-`1` translation entries of limited joints store π.

**Need.** A reader must know whether the value bounds the freedom, so that it can tell whether a limited rotation's range is complete without it. Rotations saved with and without limits, and with ranges of different widths, would fix what it states.

**Conflict.** Treating the value as a period or as a range bound states a meaning that one sample does not fix.

### DR-98. Persistent selection long-tail fields

**Question.** What do the u32 and u64 fields in the fourteen-byte persistent selection tail select?

**Known.** The 204-byte member retains the complete local, asset, and context identity and a present optional slot. Its extra bytes are `00`, u32 `2`, u64 `1`, and `00`. A following Design header or the stream-end boundary terminates the tail. The frame extent distinguishes this form from the 190-byte member without a new identity field. A further member form follows the context identity with u32 `2`, eleven zero bytes, and a run of u32 `255` values up to the next Design header; its owning Extrude selection group then has no admitted member.

**Need.** Independently varied tail fields and an unchanged selected entity must establish each field's meaning before the grammar admits other values.

**Conflict.** The tail values alone do not assign a new selected entity or an additional member.

### DR-99. Sketch ordinal collection frame tail

**Question.** What fields follow the counted sketch-member run in a Geometry ordinal collection frame?

**Known.** `f3d.md` §3.1 "The current sketch entity container" defines the collection prefix, property block, counted local-reference run, and reciprocal sketch membership. The counted run does not consume the frame. The remaining bytes include nonzero values.

**Need.** A complete field grammar must identify the tail boundaries and reference roles before a writer can construct the full collection frame.

### DR-107. Fillet face groups with several radius sets

**Question.** Which radius set does a role-`0x0000001000000000` face group use when a Fillet scope has more than one radius set?

**Known.** `f3d.md` §3.1 "A Fillet selection set can store its face selections" defines the face group and its radius when the scope has one edge group, one face group, and one constant radius. With one radius per group, `f3d.md` §3.1 "Within a scope of the Fillet family" pairs groups and radii in scope-reference order, and that pairing gives the face group the radius at its position. Groups and parameters store no assignment cross-reference. A scope with several role-`0x0000000800000000` edge groups, one or more role-`0x0000001000000000` face groups, and one radius and tangency weight per edge group pairs the edge groups with the radii in scope order; a face group then has no pairing, because the groups outnumber the radii.

**Need.** The scope-order pairing must be confirmed or replaced for face groups before a Fillet with several selection sets and face selections transfers its radii as authored.

### DR-108. Sketch scope references without a record

**Question.** What did a Sketch scope reference name when the Design stream contains no record with that index?

**Known.** `f3d.md` §3.1 "A parameter scope is one logical indexed record" defines the ordered reference table. A Sketch scope can name record indices that occur in no indexed header and in no other record of the stream. Its other references name records that only the generic indexed header classifies. `f3d.md` §3.1 "An owner frame whose stored scope record is absent" defines detached parameter owners, which remain after their dimension is deleted.

**Need.** A rule must tell a deleted sketch member from a missing record before the scope can pass structural validation without its absent references.

### DR-101. Fixed typed occurrence path discriminator

**Question.** What binding does the path discriminator of a fixed typed local component occurrence select?

**Known.** The u32 path discriminator follows the typed definition reference at offset 75. The path terminator at offset 79 and the occurrence ordinal at offset 80 follow it. The definition references, owner, and stored placement identify the local occurrence independently of a neutral interpretation of this word.

**Need.** The discriminator's target domain and binding rule are needed to retain its meaning in a component path.

**Conflict.** An occurrence ordinal does not state a path-discriminator binding.

**Note.** The fixed native occurrence record retains the discriminator value without a component-path binding.

### DR-130. Construction-operand group closing flag

**Question.** What does closing flag `1` in the tail flag block of a construction-operand group select?

**Known.** `f3d.md` §3.1 "Every `Extrude`, `Extrusion`, `Fillet`" defines the group tail. The flag block ends with the closing flag after `variant`. The closing flag is `0` or `1`, and both values occur in the three-byte block `[1, 1, closing_flag]` of existing-body groups in Extrude scopes. The role, members, and references of the group are the same for both values.

**Need.** A writer that generates a group must choose the closing flag, and a reader must know whether value `1` changes the group's operation semantics.

### DR-104. ACT component-link root selection

**Question.** Can an ACT segment contain only non-root component links, or does another reference identify its document-root link?

**Known.** `f3d.md` §3.1 "An **ACT component link** payload" defines the complete component-link frame. The tracked-entity field is a record reference. A link can close under this frame without its tracked-entity reference having value `3`. Exact frame closure does not establish the link's document-root role. An ACT stream can hold one link that tracks a component-type entity other than `3` while the Design component type has many entities and no entity `3`.

**Need.** A join between the ACT references and the Design root component must name the root among several component-type entities when a stream holds more than one link and the component type has no entity `3`.

**Conflict.** No complete root-selection rule is defined.

### DR-145. Extrude selection member scope tail

**Question.** What do the f64, the u32 with value `0x104`, and the two chain references in the 50-byte scope tail of an Extrude selection member mean?

**Known.** The tail follows the 190-byte member frame when the optional slot is absent. The u32 has value `0x104`. The two chain references name indexed records of the same segment that follow the member. The closing marked reference names the owning Extrude scope.

**Need.** Files that vary the Extrude options and the selected profile, to find which option writes the tail and what the chain records carry.

**Conflict.** None.

**Note.** None.

## 2. External references

### DR-111. Component-path poses of nested occurrences

**Question.** How does a component-path record with more than two path elements join to a nested occurrence, and does its world pose supersede that occurrence's own placement?

**Known.** A two-element path of a cross-document element and a local element naming an occurrence-placement record stores the current world pose of that placed occurrence. A two-element path of a Component record and a local occurrence carrier of that component stores the current pose of a local occurrence in the root component. Longer paths continue through further cross-document and local elements, each with an instance discriminator, and also store a world transform. A path of a referenced design's root component and occurrence placement, then this document's placement of the external occurrence and some of its local owners, stores the world pose of the inner occurrence; when the records of one instance that list the most owners agree, they place it. A path of a referenced design's root component, then this document's placement of an external occurrence and one or more of its local owners, stores that external occurrence's world pose; it places an instance that no contained record places.

**Need.** Why an external occurrence's own two-element record, and a contained record that lists fewer owners, can keep an earlier pose while the records that list more owners give another; the join from each later path element to the placement record of the nested occurrence in its placed design, and the relation between that world pose and the nested occurrence's own placement and joints, and whether a two-local-element pose of an occurrence below another component is local to its container or in the world.

**Conflict.** None.

**Note.** The contained and own forms above place occurrences (`f3d.md` §1.4). Other longer component-path records do not place occurrences; their bytes stay in the source stream.

### DR-113. Solved placement without a bound assembly operand

**Question.** Which native relation, if any, selects the current solved pose of an inserted occurrence when no assembly operand path binds to it?

**Known.** `f3d.md` §1.4 defines stored occurrence placement and component-path pose precedence. Assembly operand paths can select occurrences whose solved poses differ from their stored transforms. Operation-table type `549FBB80-B890-473E-A5C0-415D3D9BF4E6` has version-0 and version-1 forms. A typed one-element path identifies the affected occurrence. A parent descendant-path table can equal a child table's final Position matrix while the child's stored matrix equals its insertion entry. The direct table's final entry is a property of the occurrence inside its component definition, common to every instance. Component-path world poses constrain the containing instance. An instance-specific edit of the shared child violates this relation. The direct version-0 and version-1 grammar and precedence are in `f3d.md` §1.4.

**Need.** Bind each operation-associated transform to its typed occurrence path, coordinate frame, and saved state. Establish its precedence relative to occurrence-placement and component-path transforms. Distinguish a contained placement change from a containing placement change. Identify the operation suppression state and saved timeline marker before selecting an entry. Independently authored saves must change one placement or one suppression or timeline state and retain the complete occurrence, operation, and constraint carriers.

**Conflict.** A stored placement can disagree with the checked assembly while no projected joint selects the occurrence. This does not distinguish an unbound constraint, an unread placement carrier, and a different saved assembly state.

**Note.** The root, local, contained, and owned component-path forms in `f3d.md` §1.4 keep their precedence. The remaining Need applies to other path forms, incompatible constraints, and saved-state selectors (DR-111). Direct one-element tables do not select suppression or an active timeline marker. A definition with incompatible component-path and table constraints keeps its stored pose for every instance; the operation table stays native. Other path forms and saved-state selectors remain unresolved.

### DR-109. Legacy external placement control and extended envelopes

**Question.** Which native object does the auxiliary placement UUID and cross-segment metadata reference identify? What does the nonzero control word select, and how do alternate member preludes and change-category lists delimit the placement?

**Known.** The zero-control envelope has an auxiliary metadata UUID, a source occurrence key, a cross-segment reference, equal legacy targets, a stored transform or identity flag, an occurrence role, and a local owner. Other short envelopes have a different sequence between the attribute block and the repeated target. A separate envelope contains the ordered names `Appearance`, `Geometry`, `PhysicalMaterial`, `Properties`, `StructureChange`, and `Transform`, local references, and a local occurrence name before further placement members.

**Need.** A typed join through the metadata reference's segment namespace, the auxiliary UUID's relation to that target, the control word's effect on placement, and complete grammars for the alternate member preludes and change-category envelope.

**Conflict.** None.

**Note.** Auxiliary metadata remains in the complete source frame with `xref.placement-metadata-native`. The nonzero-control and alternate envelopes remain `xref.placement-undecoded` losses.

### DR-100. Rigid-group operand forms and root identities

**Question.** Which native identities do the two identity GUIDs of a zero-count root operand path name? What does the payload of the two- or three-byte field before the final ordinary-tail reference mean? Is there a separate selector for the optional tail reference?

**Known.** The typed root operand path stores a zero count, the zero root key, a zero word, u64 value 1, two ordered identity GUIDs, u32 value 2, and an eight-byte zero tail. The complete frame is 277 bytes. `f3d.md` "Assembly operands" defines every member locator form, null or nonnull, for versions 2, 3 and 5. A separate 181-byte version-2 pathless locator has a nonzero construction reference and discriminator 0; it is not a member locator. The ordinary-tail field starts with tag 0 for two bytes and tag 1 for three bytes. Its measured byte sequences are `00 00`, `01 00 00`, and `01 01 01`. The tag selects width independently of the optional reference. The reader requires unique exact closure for the reference choice; no separate presence selector is established. The ordinary-prologue byte at offset 20 is property-block presence. The canonical form stores 0 and no count; the alternate form stores 1 and a zero u32 property count at offset 21. The alternate block is present and empty. A zero separator follows it. The alternate form has its pathless reference at offset 26, its member count at offset 41, and its first member reference at offset 45; later fields shift by four bytes. Both prologues use the same ordinary tail grammar.

**Need.** The meaning of the variable-width field payload, and a separate optional-reference selector if one exists. An explicit join from each identity GUID to its owning native record or document context.

**Conflict.** None.

**Note.** The zero-count path selects the current document root. Its identity GUIDs remain native.

### DR-110. Version-4 rigid-group member locators

**Question.** What is the version-4 member-locator grammar, including construction-reference presence, transform form, and path-wrapper reference width?

**Known.** Rigid-group member references name records of type `2E3AC990-483D-4A55-8662-C064B976A957` with type version 4. The version-2, version-3, and version-5 member layouts are defined in `f3d.md` "Assembly operands".

**Need.** Exact envelope boundaries, reference widths, construction-null and identity/matrix selectors, backlinks, and counted-path closure for version 4.

**Conflict.** None.

**Note.** Unresolved member operands retain `assembly.joint-unprojected`. The group-scope prologue is defined in `f3d.md` "Assembly operands".

### DR-102. Repeated-target metadata and unresolved class-294/299 identities

**Question.** Which native identity does the first metadata UUID of a repeated-target placement name, and which namespace does the first identity GUID of an operand path admitted by class 294/299 name when its root occurrence key does not resolve?

**Known.** The placement stores two ordered LP-UTF16 UUIDs after its positive stored word. The second UUID is the occurrence key used by assembly operand paths in the source Design stream.

**Need.** An explicit join from the first metadata UUID to its native identity and containing namespace, and a document-identity join for unresolved class-294/299 paths.

**Conflict.** The second UUID's occurrence binding does not establish an identity domain for the first metadata UUID or the first class-294/299 identity GUID.

**Note.** UUID equality alone does not identify a namespace. An unresolved path admitted by class 294/299 remains native and its joint is refused with `assembly.joint-unprojected`. An ambiguous local occurrence key also refuses the joint with that loss; it does not state an external document.

### DR-103. Rigid-group member keys without occurrence carriers

**Question.** Which native relation selects the occurrence and liveness of a positive-count null-construction rigid-group member path whose root key has no local carrier or repeated-target placement carrier?

**Known.** Versions 2 and 3 admit null construction references with stored matrices or identity flags. The member locator has discriminator 2 and a complete counted path wrapper. A positive-count typed path retains its root occurrence key and identity suffix. A null construction reference also occurs with a concrete occurrence binding.

**Need.** A native relation must distinguish a current instance, an archived instance and an unresolved external instance. Controlled saves that delete, suppress and restore a grouped occurrence must establish that relation and the carrier join for a current instance.

**Conflict.** A null construction reference alone does not establish that a member is deleted or suppressed. A document identity does not select one repeated occurrence.

**Note.** The complete locator frame and ordered path remain native. A positive-count null-construction member path projects only with a concrete local occurrence carrier or placement carrier join. A member without that join contributes to `AssemblyJointUnprojected`.

### DR-82. Other joint freedom codes and the limit flags

**Question.** Which degrees of freedom do freedom codes other than `1`, `2`, and `8` name, and what do the two joint-record limit flag bytes select, and does the 444-byte `As-built` form's `SlideRest` owner hold the joint-record rest value?

**Known.** `f3d.md` "Assembly operands" gives the joint and motion records, the freedom entry, codes `1`, `2`, and `8`, and the limit lanes. Freedom codes `0`, `6`, and `7` also occur, alone and in pairs such as `2` with `7`, `0` with `8`, and `2` with `6` and `7`. Both flags are `1` on a joint with a minimum and a maximum. A slide whose joint record stores −0.5 and 1.6 has `SlideMinimum` and `SlideMaximum` owners of −5 mm and 16 mm, so translation limits are in centimetres. The 444-byte `As-built` form adds `SlideMinimum`, `SlideMaximum`, and `SlideRest` owners at local ordinals four, five, and six. Where both a `SlideRest` owner and a joint-record rest value are stored, both are `0.0`, so the stored values do not show whether the owner holds the rest value. In both 444-byte scopes, its owner stores f64 `0.0` cm and the linked joint rest lane stores f64 `0.0` cm (`0.0` mm). The two value pairs agree; the name is not the only evidence. Both joint rest flags are `1`.

**Need.** Cylindrical, pin-slot, planar, and ball joints combine freedoms. A joint with only one limit needs the flag that enables it.

### DR-83. Attributed occurrence members

**Question.** What do the two bytes before the occurrence ordinal and the marked record reference after the occurrence key of an attributed local component occurrence store?

**Known.** `f3d.md` §3.1 "An attributed local component occurrence carries a counted attribute run and an occurrence key." gives the frame. The two bytes are `00 00` in most frames and `01 00` in one. The reference names a different record in each frame, in increasing order.

**Need.** The two bytes can be a grounding or visibility state of the occurrence.

### DR-84. Occurrence identity pairs of a typed path

**Question.** What do the occurrence identity pairs after a typed occurrence path's identity GUIDs name?

**Known.** `f3d.md` "Assembly operands" gives the pair list. In one assembly, the paths of four of its six joints carry the same single pair: a decimal occurrence key and an occurrence GUID that name two different local occurrences. The paths of the other two joints, and a path continuation, carry no pairs. The pair is not the joint's own operand pair.

**Need.** A writer must know when to emit a pair and which occurrences it names.

### XR-01. `neutronData` with a different GUID

**Question.** What does `neutronData` mean when its GUID is different from the `neutronRole` GUID?

**Known.** `f3d.md` §1.4 `RedirectionsStream.dat` states that `neutronData` is an independent property. It does not need to equal `neutronRole`. The decoder keeps the value as a note string.

**Need.** We must know the meaning to build the external reference in a neutral model.

### XR-02. Non-empty `ComponentReferenceData.json`

**Question.** Which member names occur in a non-empty `ComponentReferenceData.json` object? What does each member control?

**Known.** `f3d.md` §1.4 `ComponentReferenceData.json` states that the object is an open document. It keeps member names and values without a closed schema. The decoder checks the envelope and makes no neutral projection.

**Need.** We must know the member names and their meanings to build component references in a neutral model.

### XR-04. Additional typed occurrence-placement generations

**Question.** Does a type-admitted Component occurrence placement use the component-path, matrix and closing-reference envelope carried by CommonData, or a tagged-u32 closing run?

**Known.** `f3d.md` §1.4 "**Placement.**" requires primary-index type admission. The component-path carrier has its own CommonData type; it does not establish a Component placement envelope. The legacy placement's duplicate target references, source occurrence identity, metadata UUID and five-GUID tracking join are defined. The inline `GatedByParent` field belongs to a counted typed-property block.

**Need.** A primary indexed record registered to `CE2913AA-CFE0-4F04-9102-24424ED3BCFA` must close under either proposed additional envelope before that envelope can define a placement or a tagged field.


### DR-114. Body-container Fusion owners

**Question.** How does an owner of type `074F9459-89C2-4B7D-8AB4-DCE67AE944FE` bind its Body container to a component definition?

**Known.** `f3d.md` §3.1 "The paired untyped Body-container membership" requires registered Body and Component targets. A registered version-4 Body container has a paired untyped terminal list naming Body entities 9741 and 11861 and owner entity 9709. The owner has type `074F9459-89C2-4B7D-8AB4-DCE67AE944FE`, base type `63D6AD42-466A-40B7-9CCD-EB21BC500EF6`, version 1, module `Fusion`.

**Need.** A typed reference chain from this owner to a Component definition, with the containing record boundaries and registrations.

**Conflict.** None. The Fusion owner is outside the direct Component-owner grammar.

**Note.** The body identities and terminal list are defined; their component-definition join is unresolved.

### DR-116. Joint-placed identity occurrences of inserted designs

**Question.** Which native relation binds a joint to each inserted-design occurrence that stores the identity, and in which frame does that joint define the solved pose?

**Known.** `f3d.md` §1.4 "The matrix is a proper rigid motion." defines stored identity placement and the component-path world-pose carriers. A joint names an occurrence directly through an occurrence container, or names an inserted design through an external operand container, leaving the neutral tie unresolved; each distinct external document then accounts for at most one unnamed identity occurrence. The decoder counts these occurrences, reports `assembly.joint-placed-occurrence-unsolved`, and places them at the identity with the joint unsolved.

**Need.** An inserted design whose occurrence stores the identity and whose joint solution is known must name the binding relation (direct occurrence reference or external-document container) and the frame of the solved pose, before the decoder can place these occurrences from their joints.

**Note.** DR-113 covers solved placement without a bound assembly operand generally; this joint-to-occurrence binding is a separate question.

### DR-119. Entity-cluster component-ownership fallback

**Question.** When no admitted Body-container membership names a solved body, which entity cluster selects its component owner, and when does that selection disagree with the authored ownership?

**Known.** `f3d.md` §3.1 "The paired untyped Body-container membership" defines the admitted membership join. A solved body binding without that membership takes entity-cluster component ownership, and the decoder reports `assembly.component-body-owner-fallback`. Owner-graph cycles and expansion-limit drops leave occurrences unplaced under `assembly.local-occurrence-owner-cycle` and `assembly.local-occurrence-expansion-limit`.

**Need.** Controlled saves that delete or omit Body-container memberships while holding entity clusters fixed must show which cluster the fallback selects and whether the selected owner matches the authored component, before neutral ownership can rely on the fallback.

**Note.** DR-75 defines the local occurrence carrier GUID; the validity of this fallback is a separate question.

### XR-05. F3Z member cycle, absence, and decode-failure paths

**Question.** Do the cycle, absence, and decode-failure losses exhaust the unresolvable member states of a multi-document merge?

**Known.** `f3d.md` §1.5 "Reference resolution is member-local and recursive." defines member-local recursive resolution. A member path that recurs in the active chain reports `xref.cycle` and stays unresolved. A member absent from the archive reports `xref.member-missing`. A present member that is not an F3D document member, that cannot be scanned, or that fails to decode reports `xref.member-undecoded`. Unscannable members are marked unreadable at scan time and skipped at merge time with that scan-time mark as their record. Resource-limit failures return an error.

**Need.** A merge that must continue past each unresolvable member needs each such state mapped to its loss, so the mapping covers every unresolvable state and every skipped resolvable member.

**Note.** The occurrence keeps its placement while its member stays unresolved; the merge continues with the remaining members.

## 3. Material assets

### MA-04. Texture map-channel values

**Question.** Which integer namespaces do the map-channel and advanced UVW-source properties use, and which source values enable a numbered UV set?

**Known.** The texture schema defaults are `texture_MapChannel = 1`, `texture_MapChannel_ID_Advanced = 1`, and `texture_MapChannel_UVWSource_Advanced = 0`. The physical pair does not identify a unique logical assignment under MA-08. The 3ds Max `Texmap` API defines a source enum and a numbered channel contract for that API; it does not identify the serialized namespace of these F3D properties. Matching property names and defaults do not establish that identity.

**Need.** Producer documentation, a serializer trace, or a direct property-to-wire mapping must establish the F3D or Inventor namespace and its active-source rule. Controlled saves must vary the advanced channel and coordinate source separately, verify their values after reopen, and resolve the MA-08 member assignment before a neutral UV-set binding is asserted.

### MA-08. Omitted texture map-channel member

**Question.** Which member does a `TextureMap2dSchema` closure omit from its value block?

**Known.** `f3d.md` §3.2 "An `InstanceProperties` record opens with" defines four exact omission branches: `swatch`, `texture_MapChannel`, `texture_MapChannel_ID_Advanced`, and `texture_MapChannel_UVWSource_Advanced`. Each can retain the same physical pair and the same later boundaries. Only two preserve every schema default. A default mismatch does not exclude a physical layout. A terminal `version` omission can also consume a Bump Map record exactly, but changes the later property alignment and does not define a common suffix-preserving branch.

**Need.** Controlled producer saves must vary the advanced channel ID and UVW source, preserve and verify their logical values after reopen, and identify the serialized member through a writer trace or equivalent invariant. A nonempty `swatch` control must separate its length from the adjacent link flag. Changed bytes alone can reflect mirrored or private backing values. A writer must establish the logical-to-physical assignment before it changes the ambiguous region.

**Note.** The compatibility decoder selects advanced-ID omission. This selection is not proof of the physical omitted member. Neutral map-channel interpretation remains subject to MA-04.

## 4. T-splines

### TS-05. Form selection-group scalar and flags

**Question.** What unit does the compact selection-group scalar use, what allocates its ordinal, what does the flag variant mean, and what selects the two- or three-byte flag block?

**Known.** `f3d.md` §3.1 "A compact counted selection envelope" defines the complete member run, repeated u32 ordinal, finite nonnegative f64 scalar, independent first tail reference, flag block, next-record reference, and owning-scope reference. The Form members are typed tracking records, as defined in §1.1.1. Counts alone do not identify cages.

**Need.** Controlled exports that vary evaluation time, operation order, and the relevant Boolean state must separate the scalar unit, ordinal allocation rule, flag meaning, and flag-width selector. A neutral Form writer also needs an explicit tracking-member-to-cage identity join.

### TS-06. Radial deleted-slot indices and nondefault sweep

**Question.** Does a radial operand address a raw program slot or a compact live-element index when its domain contains a deleted slot, and how does a nondefault sweep change the rotation?

**Known.** `f3d.md` §1.1.1 "In a domain with no deleted slots" defines the same-numbered neutral binding and selector-specific element domains. Raw-slot and compact-index predictions agree on all-live topology. An out-of-range operand supplies no discriminator. A raw pair remains retained when the predictions disagree. At sweep `1`, the forward and reverse rotation steps are `+2*pi/N` and `-2*pi/N`; the native sweep scalar remains separate.

**Need.** A producer-generated radial TSM must retain a deleted domain slot at `p` and map a live element with raw slot `q > p`. The raw-slot prediction is `q`; the compact-index prediction after one deletion is `q-1`. Saving must preserve the deleted slot to separate these predictions. A controlled radial export with a nondefault sweep must establish the sweep-to-angle rule.

### DR-160. EdgeFlange corner controls

**Question.** Which fields select corner relief, miter, and the corner gap for adjacent flange edges?

**Known.** The two-edge distance form names two edge wrappers, two edge groups, two aggregate operands, and one settings record. The settings input mask can be zero. The operation contains one height and angle owner pair.

**Need.** Controlled changes to each corner control with the same selected edges.

**Conflict.** None.

**Note.** The fixed edge list does not identify an independent corner-control field.

### DR-161. EdgeFlange scope radius control

**Question.** What physical quantity does the positive f64 scope radius control represent?

**Known.** The two-edge scope contains one positive f64 control at the `inside_bend_radius` layout slot. The native operation retains its value and location. The inside bend radius is the smaller radius of the coaxial inner and outer bend carriers.

**Need.** Controlled changes that separate sheet thickness, inside radius, and centerline radius.

**Conflict.** None.

**Note.** The layout slot name does not establish the control's physical role.

### DR-117. Current mixed face and distance extent payload

**Question.** Which reference assignments and orientation fields define a first-side face termination with a second-side distance, and what does target option zero select?

**Known.** The current seven-slot prologue can carry direction two, first-side extent two, and a payload second-side extent one. Target option zero also occurs with a payload second-side extent one or two. The final zero word before the scope reference count does not identify the second-side termination.

**Need.** Controlled face-then-distance changes with both direction states and independent side tapers.

**Conflict.** None.

**Note.** The scope and ordered references stay native with `feature.extrude-extent-native` until the complete assignment is defined.

### DR-118. Split tool face without a tool group

**Question.** Which tool does a `Split` scope select when reference zero is a direct face operand record instead of a counted tool group?

**Known.** The face operand has a complete `face_recipe_data` frame and an exact scope reference at reference zero.

**Need.** Controlled Split changes that select a face as the tool, with one and with several target bodies.

**Conflict.** None.

**Note.** The face operand record is validated as a frame. The Split operation keeps its existing projection rules.

### TS-07. T-spline cage decode-failure taxonomy

**Question.** Which cage program failures does `tspline.cage-undecoded` cover?

**Known.** `f3d.md` §1.1.1 "A `TSplines.BlobParts/*.tsm` entry is a UTF-8" defines the cage program. A cage entry that fails to parse transfers no surface and reports `tspline.cage-undecoded` with the entry name and the error. Untyped records and unresolved radial pairs have their own losses and remain outside this item.

**Need.** Cage entries that fail at each program stage must show which failures this loss covers, before a reader can distinguish malformed cages from unsupported program generations.

**Note.** This item is minor: the B-rep stream retains the committed face geometry of an undecoded cage.

### DR-121. Emboss controls and extended selections

**Question.** What do the Emboss control words and native pairs mean, and which values select material effect, direction, placement, and contour membership?

**Known.** The prefix has one or two control words, a counted run of native pairs, four scalar-owner links, two selection-group links, an auxiliary-group link, and the common ordered reference table. The table indices are in increasing order; prefix references bind by record identity. A sketch entity link identifies a Sketch but does not identify its selected contours.

**Need.** Independent files that change one effect, direction, placement, or contour selection at a time; complete operand frames for the extended reference table.

**Conflict.** None.

**Note.** Keep controls and extended selections native. A neutral Wrap requires a proven effect, selected profile, target face, and placement. The existing Wrap operation has no independent horizontal, vertical, or rotation controls.

### DR-120. Align operand roles and motion

**Question.** Which referenced constructions identify the moved objects, the source and target frames, and the direction of the resulting motion? What do the prefix codes and the constructor codes and marked words control? What do the matrix block and separate scalar represent, and which units apply?

**Known.** The Align prefix contains two references, an unsigned code, and a zero GUID. The references join to the ordered scope table. The compact and extended forms differ by one u32 word of value 1. The role of this word is not defined. The constructor contains eight marked words, five codes, sixteen matrix scalars, and one separate scalar. Its two scope back-references identify its owner, but do not assign a moved body or a motion relation.

**Need.** Controlled Align exports that change object selection, source and target frames, reversal, and angle independently. F22b supplies a body-alignment recipe.

**Conflict.** A referenced solved frame alone does not establish the motion applied to a selected body.

**Note.** Keep the operation native until each operand role and motion direction has a byte-defined rule.

### DR-144. Flag bytes of the discriminated parameter expression trailer

**Question.** What do the first four of the five flag bytes in the nine-byte expression trailer of a discriminated parameter frame state?

**Known.** The trailer is four zero bytes and five flag bytes, each `0` or `1`. The last flag byte is `1` exactly for a document parameter. Owned parameters set the first, second, or third flag byte, alone or the second and third together. Parameters with a null owner reference set the fourth flag byte, alone or with the first or third. Every frame with a set flag byte has the same remaining grammar as a frame with zero flag bytes.

**Need.** Files that change one parameter property at a time, such as favorite state, comment, or the deletion of the owning dimension, and save the document after each change.

**Conflict.** None.

**Note.** The decoder admits the flag bytes and does not project them.

### DR-126. Move transform envelopes with leading references

**Question.** In the Move envelope with leading references, is the matrix an absolute transform or a transform relative to a referenced frame?

**Known.** A Move scope can reference a matrix-bearing record with references before its form word. This envelope does not satisfy the zero prefix of the fixed rigid-matrix frames.

**Need.** Independent files that vary the referenced frame and the transform separately, and files that vary the form word.

**Conflict.** None.

**Note.** Keep this envelope native. Emit `feature.move-unprojected` when no admitted transform defines the Move feature.

### DR-124. Trim operands in typed profile curve entries

**Question.** How do the three u32 operands of a typed profile curve entry select a partial curve when one source loop repeats a curve identity?

**Known.** The identity record stores two region lists. Each region has counted boundary loops with exterior flags. Each curve entry stores a primary identity, a secondary identity, and three u32 operands. A complete neutral loop match or a unique atomic arrangement boundary program resolves without a partial-curve interpretation.

**Need.** Controlled selections of adjacent regions with one shared curve, and of intersecting curves with distinct selected regions, to bind each operand to curve orientation and trim bounds.

**Conflict.** None.

**Note.** A repeated identity within one source loop does not prove a complete neutral loop. A program without one unique atomic arrangement match retains its native identity and loss.

### DR-125. Counted face-target recipe without a unique historical face

**Question.** Which counted bounded-face recipe clauses select a termination face when persistent references retain several preceding support faces?

**Known.** A termination group identifies the face operand. The operand retains its counted recipe nodes, ordered prefix references, and state-local support contexts. A unique preceding face or convergent support context supplies a historical selection.

**Need.** Controlled face-target selections that vary the selected planar face while the body topology and other prefix references remain fixed.

**Conflict.** None.

**Note.** Candidate membership, planar support, and absence of changed faces do not by themselves identify a selected face.

### DR-122. Derived Base Feature source and frame

**Question.** Which field of a `Derived Base Feature` scope identifies the derived source body, and which field stores its local frame or mirror plane?

**Known.** The 711-byte frame stores nine typed-reference elements, a zero GUID, the kind, the ordinal, and an f64 of unknown meaning. The second body element of the 711-byte frame stores the Design body entity suffix of the result body. Every reference in the scope is a Design record of the same stream. The frame stores no matrix. The `DerivedBaseFeature` forms (707 and 491 bytes) store the same elements and roles, a part with a mirror label places its occurrence with an identity rotation, and no field in either form stores a plane or a matrix. The 761-byte frame has one additional typed-reference element with a role GUID that no 711-byte frame stores. Its record index is lower than the scope record index.

**Need.** A scope whose source document or source body changes with the result body fixed. The meaning of the f64 and of the additional 761-byte element.

**Conflict.** None.

**Note.** A scope without proven result bodies remains native with `feature.derived-base-unresolved`. A scope with proven result bodies is kept as a stored snapshot with the same code until source and frame have a complete binding.

### DR-123. Typed-reference BoxPrimitive offsets and other forms

**Question.** How does a nonzero `OffsetDistanceX` or `OffsetDistanceY` owner combine with the stored frame in a `BoxPrimitive` scope with eight typed reference members, and which other frame lengths does the form have?

**Known.** The 1295-byte form stores the result operation, a rigid local frame, and the five box parameter owners; the frame is the box placement when both offset owners are zero. The support-face selection and placement construction references are table members, and the form stores one further typed reference at offset 517 that is not a table member.

**Need.** A scope in this form with a nonzero offset owner and a changed placement, and a scope in any other frame length.

**Conflict.** None.

**Note.** A scope outside the 1295-byte form, or with a nonzero offset owner, remains native with `feature.box-primitive-unresolved`.

### DR-147. Region extent of a tracking-path Extrude selection

**Question.** Which Sketch region does an Extrude selection select when its member follows a tracking path?

**Known.** The path stores the primary curve, a one-based fragment ordinal, the fragment count, and the two curves bounding the fragment. The carrier lane is `2` or `3`. The selected fragment can border two bounded Sketch faces.

**Need.** Controlled selections of one region, of two adjacent regions, and of a region with an enclosed island, each in a Sketch whose curves cross, to bind the path to the selected side and to the filled island. CAD Fixture Recipes F29a-F29d select each region of two crossing circles and the union of two of them.

**Conflict.** None.

**Note.** A path selection retains its native selection and loss. The selected region can be the union of several atomic arrangement faces, which no single face proves.

### DR-146. Later ordinal in a fixed identity occurrence

**Question.** What placement does a fixed untyped occurrence with identity flag `1` and ordinal greater than one select?

**Known.** Two standard 229-byte frames store ordinal `2` and identity flag `1`. Attributed identity frames retain later ordinals.

**Need.** Independent files that vary the occurrence ordinal and placement separately.

**Conflict.** None.

**Note.** Emit `assembly.local-occurrence-identity-ordinal-unresolved` and keep the occurrence out of neutral product structure.

### DR-142. Thread construction option bytes

**Question.** What do the first and second bytes of the Thread construction marker, the pitch marker, and the zero bytes before the trailer `01` select?

**Known.** The construction marker first byte is `0` or `1` and its second byte is `1` or `2`. The pitch marker is `0` or `1`. The admitted forms fix each combination they accept. In the standard-prefix compact-marker form, the run before the trailer `01` is two or three zero bytes. Every byte in that run is zero in each Thread scope that carries it.

**Need.** Thread scopes saved with one dialog option changed at a time, and Thread scopes whose marker combinations no admitted form accepts.

**Conflict.** None.

**Note.** A scope whose marker combination no admitted form accepts stays an unresolved Thread operation.

### DR-143. Thread modeled state

**Question.** Which stored field, or which result-topology change, marks a Thread as modeled geometry instead of an annotation?

**Known.** The scope stores the designation, nominal-size text, profile name, and four physical scalars. The extended standard trailer links the length and offset owners. The extended compact trailer also links its owning Hole. Resolved Thread transitions replace existing face revisions.

**Need.** One Thread saved with modeled geometry and the same Thread with it off, with the Design stream and the ASM history of both. CAD Fixture Recipes F17a and F17b specify this pair.

**Conflict.** None.

**Note.** The neutral thread leaves `modeled` absent.

### DR-148. Target references in populated configuration tables

**Question.** How does an authored configuration table encode its driven parameter and feature targets, row identities, and cell values?

**Known.** The table entries in seven public parametric hardware designs and 37 recipe uploads contain the JSON object `{}`. Each entry has zero rows and cells. The Design `BulkStream` contains the table serializer trigger and separate `DesignConfigurationRoot` records. The existing configuration decoder admits named JSON variant objects and joins parameter override names to unique neutral parameter identities. Design parameter records carry their own indexed identities in the Design `BulkStream`.

**Need.** Legally available designs with at least two authored configurations, a changed parameter cell, and a changed feature state, with the corresponding table entries and Design records.

**Conflict.** None.

**Note.** Unassigned JSON members retain their native values and emit `configuration.member-unassigned`. The neutral configuration arena supports parameter overrides and evaluated feature states.

### DR-149. Text-bearing document parameter prefix

**Question.** What fields delimit the document parameter prefix that contains an additional text label before its source ordinal?

**Known.** Two user-parameter frames in the public parametric standoff design store text labels before the ordinal, null owner, expression, source kind, unit, and name. Their tails reference the registered parameter-tail type. The fixed discriminated-prefix reader refuses the frames and reports `parameter.record-unparsed`.

**Need.** Independent designs with labeled user parameters and controlled changes to the label, source name, and expression.

**Conflict.** None.

**Note.** The source archive retains the complete frames.

### DR-152. Mirror selection run with role 0x10

**Question.** What do the members of a Mirror scope's role-`0x10` group select?

**Known.** `f3d.md` §3.1 "A Design Mirror scope" lists the seed and plane groups. Mirror scopes in the public native-assembly drive-unit design also store a counted role-`0x10` group whose members are class-256 persistent selection records. The same scopes store a role-`0x5` plane group with one bounded-face recipe member. These scopes do not decode a mirror construction.

**Need.** Mirror features with controlled changes to the selected objects, to find which seed kind the group carries and how it relates to the seed group.

**Conflict.** None.

**Note.** None.
