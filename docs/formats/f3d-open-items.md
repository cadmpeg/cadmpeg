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

### DR-463. Analytic sketch-curve tail flag frames

**Question.** Which analytic sketch-curve tail frames carry the construction flag, and how does the type version select each frame?

**Known.** `f3d.md` §3.1 "Immediately after the 96-byte analytic payload, a planar sketch-curve record stores" describes one tail frame. The two-reference tails of the six public regression inputs start after the references with three zero bytes, byte `1`, a variable Boolean byte, and five or six further bytes before a u32 `1`. Eight curves reported as unprofiled have that variable byte set: two curves in z_mounts_front and six in standoff. The current reader's guard does not admit these frames. The same tail question affects the public HexGrid, Wago, and both spool-mount inputs. HexGrid Sketch 245 has rectangle diagonals 278 and 279 at record offsets 110628 and 110935. Both spool-mount files have Sketch 265 diagonals 126 and 127 and divider 131; their two-reference tails contain the variable Boolean byte set to one. In the dual mount their records start at offsets 96792, 97095, and 99424; in the single mount they start at 80997, 81300, and 83629. Wago Sketch 187 rectangle curves 105 through 108 start at offsets 54038, 54345, 54652, and 54959 and carry the same variable byte. These curves participate in the current profile arrangement.

**Need.** A type-version mapping and controlled construction/reference toggles that identify the Boolean byte and the complete tail boundary for each frame.

**Conflict.** The specified guard position differs from these stored tails.

**Note.** The decoder retains each curve and applies the admitted construction-tail rule. Endpoint incidence and geometric closure remain independent of the unresolved Boolean.

### DR-319. WorkAxis point-and-direction construction rules

**Question.** What do the position, direction, construction rules, and input roles encode for the other `refType` values of a WorkAxis point-and-direction carrier?

**Known.** `f3d.md` §3.1 "A `WorkAxis` scope can reference a point-and-direction carrier" describes the carrier and the two-point rule.

**Need.** Controlled designs that vary one axis construction rule and its inputs at a time.

**Conflict.** None.

**Note.** The decoder takes position and direction as the solved axis for other values and reports `feature.work-axis-rule-unassigned`; the construction inputs remain native.

### DR-284. Compact sketch-profile tail flags

**Question.** What do the two compact sketch-profile tail flags encode, and how do they affect profile selection?

**Known.** `f3d.md` §3.1 "A sketch-driven `Extrude` scope" gives the position and admitted values of the flags. The construction-operand group tail in `f3d.md` §3.1 "Every `Extrude`, `Extrusion`, `Fillet`" ends its flag block with `variant` and the closing flag, and these two bytes also immediately precede the reference to `N+1`.

**Need.** Controlled designs that vary each flag and establish the selected profile for each value.

**Conflict.** None.

**Note.** The decoder retains the flags with the sketch-profile operand, binds the profile as for flags `[0, 0]`, and emits `feature.sketch-profile-flags-assumed` when a flag is `1`.

### DR-285. Compact sketch-profile tail word and final reference

**Question.** What does the nonzero u32 of a compact sketch-profile tail encode, and which record does the final marked reference of a sketch-profile tail name?

**Known.** `f3d.md` §3.1 "A sketch-driven `Extrude` scope" gives the 83-, 89-, and 93-byte tails, the nonzero u32, and the marked references to `N+2`, `N+1`, and a third record.

**Need.** Controlled designs that vary the profile and its owning scope, with the record that carries the index of the final reference.

**Conflict.** None.

**Note.** The decoder checks that the u32 is nonzero, that the 93-byte tail repeats it, and that the final marked reference is present.

### DR-106. Hole extent leading word

**Question.** What does the u32 word before the Hole extent controls in type versions 5 and 7 encode?

**Known.** Type versions 3 and 4 omit the word, and type version 3 omits the extent control. The word can be nonzero on Distance and non-distance holes.

**Need.** Controlled Holes of type version 5 or 7 that vary the hole, tap, and position settings independently and bind each word value to one setting.

**Conflict.** None.

**Note.** The decoder retains the word on the native extent and in Hole source properties. A nonzero word reports `feature.hole-extent-leading-word-assumed` for the reading that the stored scalar geometry remains independent of it.

### DR-220. Hole carrier placement forms

**Question.** Which placement does a Hole carrier state when its `refType` is not `13`, `14`, or `19`?

**Known.** `f3d.md` §3.1 "The carrier `refType` states how" defines the three admitted forms. The point-data record `refType` values `5`, `6`, and `17` occur on carrier `refType` `19` and do not change its placement.

**Need.** A file with a Hole carrier of any further `refType`.

**Conflict.** None.

**Note.** Such a Hole retains its native placement and reports `feature.hole-placement-unproven`.

### DR-221. Primary identity of a Hole carrier selection

**Question.** What does the primary persistent identity of the direct selection on a Hole carrier name?

**Known.** `f3d.md` §3.1 "The direct support-face selection has type GUID" gives the frame. On a carrier with `refType` `13` the identity can match a loop in a state after the Hole, so it does not name the support face there. The transition states the support face in `f3d.md` §3.1 "The transition that drills a Hole".

**Need.** Paired Holes that vary the sketch point, the sketch, and the sketch plane while the support face stays fixed.

**Conflict.** None.

**Note.** The selection stays a native record. A candidate from this identity binds only when the transition states no support face.

### DR-09A. Sheet-metal settings compatibility and override encoding

**Question.** What is the Hem Tangent bend-position code, what do the compatibility values `1` and `4` encode, what does the version-3 rule-reference suffix encode, and how do the settings mask bits, optional slots, and override bundle tails map to independent rule controls?

**Known.** `f3d.md` §3.1 "The common Hem controls start" defines Adjacent code `3`, Flip `0`/`1`, the two null legacy references, and the form-specific owner layouts. "The common sheet-metal settings carrier" defines the versioned optional-reference runs, typed rule and presentation links, feature owner, paired header, and context. Mask `4` with slot 1 reaches BendRadius; mask `240` with slot 2 reaches BendWidth, BendDepth, and BendRemnant. The version-3 rule-reference suffix is `00 00` or `01 00 00`; its byte form remains native. Those joins do not assign each mask bit a meaning or define the suffix control, remaining slots, and bundle tails.

**Need.** A controlled Tangent Hem and one-control-at-a-time rule overrides must separate the bend-position code, compatibility lanes, individual mask bits, and optional slots. Paired controls must distinguish the two version-3 rule-reference suffix forms. A complete bundle grammar must account for each byte after the verified owner prefixes before a writer generates those bundles.

### DR-12. Placement `refType` values

**Question.** What construction does each `refType` value of the placement class select, how does a nonzero `extraOffset` alter that construction, and what does the version-16 `periodicFaceMode` select?

**Known.** `f3d.md` §3.1 "A `Sketch` scope joined" gives the member order, the version gates, the identity-marked matrix, and the unmarked version-4 matrix. The independent `extraOffset` reference reaches a scalar owner linked to an `ExtraOffset` length parameter. `refType` is a u32 whose value does not change the member sequence. The scalar-role table defines the ordered `values` names and direct-reference shapes. Owner-scope references remain independent of the direct geometry slots, and the solved matrix does not select the construction rule. Code `15` has a `RotateAngle` scalar and two face-selection lineages; those operands do not distinguish the exact plane-construction subtype.

**Need.** We must know which references a given `refType` requires to write a placement that Fusion re-solves rather than accepts as stored. A controlled nonzero `ExtraOffset` must establish its direction and effect for each supported construction. Nonzero `isZFlip` and `axisFlipped` bytes, `bUseUDirection` values above one, and version-16 periodic-face modes require a solved-frame orientation rule. An offset-plane definition also requires the scope operand role that selects its reference plane or planar face; a solved matrix and an `AlongDistance` parameter do not assign that role.

### DR-14. `SurfacePatch` boundary-side ordinal mapping

**Question.** Which `PatchFlip` ordinal selects each continuity-direction Boolean and incident face?

**Known.** `f3d.md` §3.1 assigns `PatchFlip` to the per-boundary u32 at `R + 26` and defines `PatchScale` as the continuity weight. Raw side ordinals `0` and `2` remain distinct. The version-7 scope's whole-surface normal Flip is independent of this ordinal.

**Need.** One supported edge with two distinct incident faces must be authored with each continuity-direction Boolean while continuity, edge order, and weight remain fixed. The saved ordinals and the API's first co-edge face must identify the side mapping.

### DR-317. `SurfacePatch` interior groups

**Question.** What geometric condition does each interior group of a `SurfacePatch` scope impose on the patch? What do role-`0x0000002100000000` groups of the scope that the interior list does not name select?

**Known.** `f3d.md` §3.1 "A `SurfacePatch` scope in the interior-group form" gives the interior list, the group role, and the member runs. The role-`0x0000000400000000` boundary groups form the complete closed boundary without the interior groups. Interior members are edge recipes, sketch-curve entity selections, and point entity selections, and a group can mix curve and point members. An interior group has no settings record. A scope reference table can also hold role-`0x0000002100000000` groups with the same frame and member grammar that the interior list does not name. The decoder projects each interior group as one `FilledSurface.interior_constraints` path, in interior-reference order, and reports `feature.surface-patch-interior-constraint-assumed`. It treats unnamed groups as inactive and reports `feature.surface-patch-unnamed-groups-assumed-inactive`.

**Need.** A controlled design must add one interior curve and then one interior point to a fixed boundary, save each state, and compare the patch face geometry with each interior selection. A second design must remove an interior selection and save again, to show whether an unnamed group is a retained former selection.

**Conflict.** The bytes identify selections but do not establish their geometric condition.

**Note.** The decoder charges a loss for the projected interpretation.

### DR-318. `SurfacePatch` selection-prefix words

**Question.** What do the u32 `1` at selection-prefix offset 4 and the zero bytes before the interior count and before the tool-body reference encode?

**Known.** `f3d.md` §3.1 "A `SurfacePatch` scope of type version 5, 6, or 7" gives the positions. Versions 6 and 7 have four more zero bytes in each of the two zero runs than version 5. The decoder reads the interior and tool-body references only when the u32 is `1` and each zero run is zero.

**Need.** Designs that vary the patch type, the boundary selection mode, and the interior selection kind must show which option writes a nonzero value in each position.

**Conflict.** The field values do not identify their meanings.

**Note.** The decoder validates these values before it reads the references.

### DR-19. Construction-group fields

**Question.** What do the construction-group scalar fields hold? What does the ordinary variant byte control? What allocates the ordinal? What does compact relative-placement `false` mean, and what distinguishes the two dual-transform slots?

**Known.** `f3d.md` §3.1 defines the feature-specific roles, the Sketch retained-input role `0x81 << 32`, and the pattern path-carried seed role `0x100 << 32`. The group holds a nonzero u32 `ordinal`, a nonnegative finite f64 `scalar`, and a second copy of `ordinal` that one container generation omits. The ordinary `variant` is zero or one; the class-gated legacy body-tail pairs have separate grammars. `ordinal` does not encode the timeline feature ordinal, and Sketch groups can have different ordinals in one scope. Its allocation event and the unit of `scalar` remain unsettled. The two auxiliary references retain ordered paths. A single matrix stores their relative placement when both rigid path placements are defined; compact `true` represents identity placement. A missing path pair supplies no placement equation. A dual carrier retains two matrices in order. The full-round Fillet form supplies its center-face selection independently of the relative-placement carrier.

**Need.** Controlled evaluation and save pairs must distinguish the scalar unit, ordinal allocation event, and ordinary variant meaning. A compact-`false` control must separate invalid, default, identity, and inactive interpretations. Independent occurrence and local-geometry edits must distinguish the dual slots. Auxiliary-path forms without an admitted rigid placement need complete typed grammar before they define a relative-placement equation. Preserve these fields and forms until those distinctions are established.


### DR-261. Different construction-group ordinals in one SurfaceOffset scope

**Question.** What do different construction-group ordinals mean when the support groups of one grouped `SurfaceOffset` scope carry them?

**Known.** `f3d.md` §3.1 gives the construction-group payload and its `ordinal`, and the grouped support form of `SurfaceOffset`. The groups of one feature scope usually share one ordinal; a grouped `SurfaceOffset` scope stores 252 in one document and 260 in another. DR-19 records that the ordinal's allocation event is unsettled. The decoder reads groups with different ordinals as one support-face selection and reports `feature.surface-offset-group-ordinals-assumed`.

**Need.** A save of one `SurfaceOffset` whose support faces come from two selections made at different times, with the face set that Fusion shows for it.

**Conflict.** None.

**Note.** None.

### DR-273. Scale frame whose payload length matches no form

**Question.** Which form does a legacy `Scale` scope store when its payload length matches none of the 297-, 299- and 308-byte forms?

**Known.** `f3d.md` §3.1 keys the legacy `Scale` forms on the payload length after the two serialized bytes per UTF-16 kind code unit are excluded: 297 bytes with five ordered references, 299 bytes with five, and 308 bytes with six. When the payload length matches none of them and the frame is 307 or 309 bytes with five ordered references or 318 bytes with six, the decoder reads the form whose English frame has that length. In a 309-byte frame it requires the null GUID, the reference table and the tail at the English offsets of the 299-byte-payload form. It reports `feature.scale-frame-length-form-assumed`.

**Need.** A `Scale` scope whose kind spelling and frame length give a payload length outside the three forms, with its factor and center checked against the result volume.

**Conflict.** None.

**Note.** None.

### DR-25. Base Feature six-byte fields

**Question.** What do the six-byte fields after the Base Feature body suffixes and after the Base Feature record references hold?

**Known.** `f3d.md` §3.1 "**References.**" resolves the six bytes that follow an eleven-byte element: they are the target entity ID's high half and the two reference flags. A fifteen-byte element already carries the full entity ID, so its six bytes are instead the two flags and a further u32 member of the element. The `u16 0` then `u32 1` form is that member holding `1`: the flags are both clear, and a cross-segment reference would put `1` in the second flag byte rather than in the u32. The value is uniform across a scope's body-entity run.

The class-360 primary paired with class-258 and the class-409 primary paired with class-262 use the same six-byte fields on their body, repeated passive-reference, and result-record entries as the ordinary Base Feature form. Their non-empty frame length is `262 + 52N`; their non-empty shared metadata field is two bytes, and the class-409 zero-body frame is 258 bytes with a six-byte shared metadata field. The class-444 primary paired with class-263 uses the same six-byte fields on its body, repeated passive-reference, and result-record entries; its passive count field has repeat marker `0`, its non-empty shared metadata field is two bytes, and its zero-body form has fourteen zero bytes after the shared metadata record. This fixes the class-keyed envelopes but does not assign a meaning to the six-byte fields.

The class-377 primary paired with class-259 is a separate direct body-reference envelope. Its body entity suffix is a u32 at primary-relative offset 40 with a ten-byte zero tail; it does not use the six-byte result-body element runs.

The class-290 primary paired with class-261 uses the same six-byte fields on its body, repeated passive-reference, and result-record entries. Its non-empty frame length is `261 + 52N`, its count field has six zero bytes before `u32 N`, and its shared metadata field is two bytes.

**Need.** We must know what the per-element u32 selects to write a Base Feature from a neutral model. It is `1` on one scope and `0` on every other, and it does not track the body count, so we cannot say what selects it.


### DR-259. Owner of a scope that several timeline objects name

**Question.** Which object owns a `Base Feature` scope that two or more paste or body-to-component objects name, and which fields bind an object without a later same-index header?

**Known.** `f3d.md` §3.1 "A paste or body-to-component timeline object owns" gives the owner fields; each object stores its reference to the owned scope after its counted body references. The owned scope takes its owner's timeline position. The decoder gives the scope to the object with the lowest record index whose owned runs read and reads no copy sources for it. For an unpaired object, it reads the count and references as the owned scope and leaves the copy form unread.

**Need.** A save in which two objects name one scope, with the timeline position and the body set that Fusion shows for that scope.

**Conflict.** None.

**Note.** The decoder reports `feature.timeline-owner-competing` or `feature.timeline-owner-unpaired` for the respective cases.

### DR-260. Placement of a pasted copy

**Question.** Where does a pasted copy sit relative to its source body?

**Known.** `f3d.md` §3.1 "In the scope that a paste object owns" gives the copy form. The object and scope frames store the copy bodies, their parameter-body records and the source GUID pairs. In the copy saves whose sources survive, each copy has its source's bounding box and volume. The decoder projects the copy as `MoveBody` with zero translation and no rotation.

**Need.** A paste that places a copy at an offset or a rotation from its source, and the bytes that hold that placement.

**Conflict.** None.

**Note.** The decoder reports `feature.paste-copy-placement-assumed`.
### DR-26. Sketch-text fields

**Question.** What do the residual `txt_tag` style bits and four-byte style payload, its second post-height Boolean, and the `textex_tag` orientation and auxiliary flags select?

**Known.** `f3d.md` §3.1 "In a `txt_tag` record the twenty-nine bytes" defines the bold bit, horizontal-flip Boolean, anchor coordinates, and versioned pre-string lane. "The `txt_tag` leading block names four frame lines" defines the leading and post-string references. "A `textex_tag` record's class tail opens" defines the cached natural width and its independence from frame width. "The legacy text presentation suffix" defines the default presentation payload and separates it from glyph colour. The first of the three bytes after horizontal alignment and the byte after vertical alignment can both be set. That paired state does not distinguish an axis-specific flip from an echoed flag. Alignment enums vary independently. The second `txt_tag` Boolean can be set with the first clear; its glyph-axis meaning remains unresolved. The numeric font weight does not assign a meaning to an auxiliary flag.

**Need.** Independently authored horizontal-only and vertical-only flips must distinguish the legacy orientation flags. An authoring control that changes the serialized style payload must identify each residual style bit or field. API text-style enum values alone do not assign serialized italic or underline bits.

### DR-27. Remaining sketch-pattern state

**Question.** What do the third rectangular-pattern flag and the positive circular closing flag select, and how are rectangular map keys assigned with two active directions?

**Known.** `f3d.md` §3.1 "The tangency subclass flags refer" defines the bounded endpoint choices and tangent-direction sign in semantic return order. "The rectangular-pattern reference run" defines the optional line-direction slots, the explicit extent flag, and direction-one symmetry. The reference count does not choose distance form. "The pattern map is an optional instance-identity cache" defines persistent curve identities, repeated groups, stale entries, circular keys, and one-row triangular rectangular keys. "The u32 run after the pattern map" defines suppression ordinals. "The text-frame class's first reference" defines its optional center point. The circular closing byte is Boolean; zero accompanies the one-direction partial-angle distribution. The format model assigns no positive-state meaning from public API property order alone.

**Need.** A positive direction-two symmetry control must bind the third rectangular flag to its serialized state. A circular pattern with a positive closing flag and a partial angular extent must distinguish symmetric from one-direction distribution. A non-empty two-direction rectangular map must identify its general instance-to-key rule. Empty maps remain valid and do not require that rule.

### DR-31. Pipe path parameterization and point-selection section frame

**Question.** Where does the section frame of a `Pipe` with a point-selection pair sit? What sets the origin and orientation of a partially consumed `Pipe` path, and does a `Pipe` whose forward and reverse fractions both equal one sweep its whole path group when its section lies inside the path?

**Known.** `f3d.md` defines the compact and legacy operation prefixes, the independent `isHollow` Boolean, the four canonical scalar owners, the settings record, the complete construction groups, the optional point-selection pair, the section frame and its section shape, and the outside-size and inward-thickness conventions. The two bytes after the operation are opaque. Forward and reverse path fractions remain exact native values. The point-selection pair does not establish a path origin. The decoder reads a `Pipe` with a new-body, join, or cut result operation as a circular section swept along its whole path group when its forward and reverse fractions both equal one, or one equals one and the other zero. A `Pipe` whose section frame stores the circular shape needs no further section test, and one whose frame stores another value stays native. Without a read shape, the decoder tests the result bodies of a new-body `Pipe` that has result bodies when no later feature names one of those bodies and every later body-changing feature names its own result bodies. The test passes when the result bodies are solids whose every face lies on a plane, on a cylinder whose radius is the outside radius or, for a hollow section, the inner radius, or on a torus whose minor radius is one of those radii, and the faces include at least one curved face of each radius. A `Pipe` that passes the test reports `feature.pipe-circular-section-inferred`, and a `Pipe` that fails it stays native. Every other `Pipe` with those fractions reports `feature.pipe-circular-section-assumed`; this includes a new-body `Pipe` followed by a body-changing feature that names no result bodies. Fractions that both equal one report `feature.pipe-whole-path-extent-assumed`. For one unit and one zero fraction, the decoder assumes that the section lies at the path end where the unit fraction starts, and reports `feature.pipe-path-end-section-assumed`.

**Need.** A `Pipe` with a point-selection pair, saved with each section shape, shows where its section frame sits. Partial-path reconstruction needs the native path parameter origin and orientation. A `Pipe` with fractions of one whose section lies inside a multi-edge path, saved with its result body, shows which part of the path it sweeps. A `Pipe` with one unit and one zero fraction, saved with its result body, shows which path end holds the section.

**Conflict.** None.

**Note.** None.

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

### DR-472. CopyPaste source occurrence identity

**Question.** Which persistent identity selects the source occurrence of a CopyPaste scope whose occurrence key is the null GUID?

**Known.** `f3d.md` §3.1 "A CopyPaste scope whose occurrence relation has an owner collection" gives the target occurrence relation and the source and target local placements. Identity flags can omit either placement matrix. A component definition can have several occurrences with equal local placements.

**Need.** Join the source placement to one occurrence and its containing component through a persistent source reference.

**Note.** The transfer retains its source placement and target operands. `feature.occurrence-transfer-source-unresolved` records the unresolved source occurrence identity.

### DR-75. Local occurrence carrier GUID

**Question.** What does the LP-UTF16 carrier GUID at offset 44 of a local
component-occurrence carrier identify?

**Known.** `f3d.md` §3.1 "The component-record reference names the component
definition record." gives the component-definition join through the
component-record reference and the naming-space binding. Two carriers that
name one component definition record can store different carrier GUIDs. The
legacy `CopyPaste` and `R-Pattern` joins select carriers by equal carrier GUID.
The compact `CopyPaste` projection compares carriers by their component-record
reference.

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

### DR-78. Symmetric Extrude distance measurement without a measurement word

**Question.** Which field states whether the distance of a symmetric distance
`Extrude` is the travel on each side or the whole travel in a layout that
stores no measurement word?

**Known.** `f3d.md` §3.1 "In the current reference-aware layout, a one-sided
direction" gives the measurement word of the current reference-aware layout.
One symmetric `Extrude` without indexed parameters stores lane zero as `1.2`
cm on a Sketch in the plane `x = 150.5` mm. Its operation history holds planes
at `x = 138.5` mm and `x = 162.5` mm, which is `12` mm on each side. The
prologue stores travel direction `3`, face-extend option `2`, and side extents
`[1, 0]`.

**Need.** Controlled symmetric Extrudes with a fixed lane or an indexed distance must establish the measurement when the word is absent.

**Conflict.** The fixed-lane reading uses the distance on each side. The single-lane indexed-parameter reading uses the distance as the whole travel.

**Note.** The decoder retains the chosen reading with `feature.extrude-symmetric-distance-assumed`.

### DR-79. Typed-reference Design record grammars

**Question.** What are the dimension-locus, construction-recipe, feature-prologue, and other fixed scope-tail grammars in the container generation that writes typed references?

**Known.** `f3d.md` §3.1 "The typed compact parameter form is the compact parameter form" gives the parameter form, and `f3d.md` §3.1 "In the container generation that writes typed references" gives the parameter-owner frames. In that generation every local reference carries its LP-ASCII 36 target type GUID, so each reference is 51 bytes instead of eleven. Sketch and feature records also store typed references. `f3d.md` §3.1 "A typed-reference parameter scope" gives the 196-byte scope tail and the typed reference table, and `f3d.md` §3.1 "A typed-reference sketch member-run record" gives the sketch container member run and the explicit head record. `f3d.md` §3.1 "A typed-reference companion prefix" gives the companion prefix, `f3d.md` §3.1 "A typed-reference Extrude scope" gives the typed and shifted typed Extrude prologues, `f3d.md` §3.1 "A typed-reference Extrude selection group" gives the selection group, and `f3d.md` §3.1 "The class-409 zero-body form has" gives the zero-body Base Feature. Typed dimension locus frames, construction recipes, and the other feature-specific fixed scope tails of this generation have no specified grammar.

**Need.** Sketch constraints and complete feature definitions in documents of this generation stay native until each fixed-offset grammar reads references by their encoded width.

### DR-89. Typed profile-identity entry fields and other profile forms

**Question.** What do the twelve bytes after the secondary identity in a typed profile-identity entry mean, what separates the two loop lists, and which form do the typed Extrude profile groups without a typed profile selection use?

**Known.** `f3d.md` §3.1 "A typed-reference profile selection is a construction-operand group member" gives the record grammar and the exact join. The twelve bytes are three u32 values; observed values are small, and the first is `0` or `1`. Some typed Extrude profile groups name members that are not typed profile selections.

**Need.** Controlled saves must vary one selected region to two selected regions, then add a hole and a nonempty second list while keeping the Sketch and feature operation fixed. Compare the zero region terminators with both counted-list boundaries. Vary each entry operand independently and compare the selected outer and inner boundaries and curve orientation. The other member form remains native until its complete grammar resolves.

### DR-80. Parameter collection fields

**Question.** What do the u32 before the marked reference, that marked reference, the final marked reference, and the type-version-4 prefix of a parameter collection store?

**Known.** `f3d.md` §3.1 "The twelve-byte parameter tail is" defines the collection and its counted parameter run. The u32 can equal the parameter count, can exceed it, and can be zero. The first marked reference names the record with the next index. Type version 4 also registers 61-byte records that no parameter tail names.

**Need.** A writer must emit these fields to build a parameter collection for a new Design stream.

### DR-81. Other typed compact parameter trailers

**Question.** What do the typed compact parameter frames with other expression trailers or a null owner on a feature input encode?

**Known.** `f3d.md` §3.1 "The typed compact parameter form is the compact parameter form" gives the owned and document forms. Some typed frames store the null owner reference with a feature-input or dimension `source_kind` (for example `HoleDepth` or `TaperAngle`) and the expression trailer `00 00 00 01 00`. Some owned frames store the expression trailer `00 01 00 00 00`. A null owner also occurs in the wide discriminator-`0` form with the document trailer and `source_kind` `Paramètre utilisateur`. The decoder reports these frames as `parameter.record-unparsed` and does not decode them.

**Need.** An ownerless feature input has no owner binding in the current parameter model, and the meaning of the set trailer bytes is unknown. A rule must bind the document trailer to a localized `User Parameter` source kind before such a frame forms a document parameter.

### DR-85. Flagged mesh-collection words

**Question.** What do the flag byte at offset 20 and the u32 at offset 25 of a flagged mesh collection store, and what does the u32 of each flagged body entry store?

**Known.** `f3d.md` §3.1 "The mesh-collection class" gives the flagged layout. The u32 at offset 25 is `1` in an empty flagged collection. A nonempty flagged collection can store its body count in this word, or store 6 or 4 with two bodies and 3 with one body. Every counted body entry stores `1`. A flagged collection can terminate its nested body-reference run with a direct owner reference.

**Need.** The value can count mesh bodies removed from the feature. A design in which mesh bodies are inserted and removed one at a time, saved after each step, establishes the meanings of these fields.

### DR-292. Mesh Design record words

**Question.** What do the mesh-GUID bytes at offsets 21 through 31, the mesh-body prefix words, and the remaining mesh-operation member fields store?

**Known.** The mesh-GUID record carries eleven bytes. A mesh body carries one prefix word; a flagged mesh body carries two. The operation member references a mesh body. The operation scope frame can also store marked same-segment references to the mesh-GUID records of input meshes, at frame positions that vary. The decoder takes every reference in the scope frame that names a mesh-GUID record with a reciprocal entry name as an input. The decoder retains the words and the operation source records and reports `mesh.guid-bytes-uninterpreted`, `mesh.body-words-native`, and `mesh.operation-records-native`.

**Need.** A design saved after individual body insert, move, and scale operations establishes the meanings of these fields. A controlled mesh operation establishes the remaining member fields.

**Conflict.** None.

**Note.** None.

### DR-293. Mesh stream metadata

**Question.** What component order does a split mesh stream use, what history bound does its compression encoding state, and what does the mesh face-group registry value count?

**Known.** A split stream stores two, three, or four equal-length f32 segments. The decoder retains its descriptor, reads the segments in stored order, and reports `mesh.split-stream-assumed`. It uses the larger of 1 MiB and the declared output length as the LZMA history bound and reports `mesh.history-window-assumed`. A face-group channel lists key and GUID entries. The decoder retains field 9 and all entries, uses the triangle selectors, and reports `mesh.face-groups-assumed` when the count differs or a group has no triangles.

**Need.** A mesh with distinct known coordinates establishes the component order. A documented encoding rule establishes the history bound. A controlled face-group edit establishes the meaning of field 9.

**Conflict.** None.

**Note.** None.

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

**Known.** `f3d.md` §3.1 "A rectangular-pattern direction record has a 220-byte, 221-byte, or 195-byte form" gives the frame, the U-then-V order, and the direction as the normalized displacement. Kind `8` selects a linear edge, with the edge start point and the edge vector. A kind-`16` record stores a unit displacement and an origin on the selected entity. The reference at offset 138 names a record outside the scope reference table.

**Need.** A writer must emit the selection kind, the origin, and the reference at offset 138 for a direction that does not come from a linear edge, and a reader must know whether a kind-`16` origin is a point of a planar face, an axis, or another entity.

**Note.** In `Parametric_extrusion_generator.f3d`, R-Pattern 3 scope `4709` has kind-`16` direction record `4736` at Design byte `135573`. Its displacement is `(0, 0, 1)`, its U distance is `-1.5` cm, and payload record `4716` supplies V displacement `(1, 0, 0)` at byte `133177`. Both counts are two and the V distance is `1.5` cm. The decoder transfers the two directions and their 15 mm spacings. The auxiliary reference names record `7609`.

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

**Known.** `f3d.md` §3.1 "A Fillet selection set can store its face selections" defines the face group and its radius when the scope has one edge group, one face group, and one constant radius. With one radius per group, the rule in DR-321 Known pairs groups and radii in scope-reference order, and that pairing gives the face group the radius at its position. Groups and parameters store no assignment cross-reference. A scope with several role-`0x0000000800000000` edge groups, one or more role-`0x0000001000000000` face groups, and one radius and tangency weight per edge group pairs the edge groups with the radii in scope order; a face group then has no pairing, because the groups outnumber the radii.

**Need.** The scope-order pairing must be confirmed or replaced for face groups before a Fillet with several selection sets and face selections transfers its radii as authored.

### DR-101. Fixed typed occurrence path discriminator

**Question.** What binding does the path discriminator of a fixed typed local component occurrence select?

**Known.** The u32 path discriminator follows the typed definition reference at offset 75. The path terminator at offset 79 and the occurrence ordinal at offset 80 follow it. The definition references, owner, and stored placement identify the local occurrence independently of a neutral interpretation of this word.

**Need.** The discriminator's target domain and binding rule are needed to retain its meaning in a component path.

**Conflict.** An occurrence ordinal does not state a path-discriminator binding.

**Note.** The fixed native occurrence record retains the discriminator value without a component-path binding.

### DR-282. Legacy body-group flag blocks

**Question.** Do the primary and paired class tags determine the frame and flag block of a legacy body group?

**Known.** Legacy body groups with primary/paired classes 257/262, 323/262, and 338/261 have flag block `01 01`; classes 328/263, 282/262, and 302/258 have flag block `00 01`. The group parser accepts both Boolean values for the variant and closing flag and closes the frame by record index.

**Need.** Controlled files that vary each flag for each class pair.

**Conflict.** None.

**Note.** The flag block is retained in the native group frame.

### DR-130. Construction-operand group closing flag

**Question.** What does closing flag `1` in the tail flag block of a construction-operand group select?

**Known.** `f3d.md` §3.1 "Every `Extrude`, `Extrusion`, `Fillet`" defines the group tail. The flag block ends with the closing flag after `variant`. The closing flag is `0` or `1` in the two-byte and three-byte blocks. The role, members, and references of the group are the same for both values. The decoder reads both values with the same group semantics, keeps the flag in the native group frame, and reports `feature.construction-closing-flag-assumed` for each group with closing flag `1`.

**Need.** A writer that generates a group must choose the closing flag, and a reader must know whether value `1` changes the group's operation semantics.

**Conflict.** None.

**Note.** Every legacy body group in DR-282 has closing flag `1` in the two-byte block.

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

### DR-154. Extended Extrude selection member tail

**Question.** What do the bytes between the 190-byte frame of an Extrude selection member and the next indexed record header carry when the tail is neither the fourteen-byte nor the fifty-byte form?

**Known.** A tail that a following header ends is 1 to 1024 bytes long. When no header follows, the tail runs to the stream end. Stored tails include five zero bytes; a UTF-16 fragment of a UUID followed by fourteen bytes; zero bytes followed by u32 values and marked references of the same segment; a second length-prefixed UUID; an unlengthed indexed record; and a run of zero bytes to the stream end. The member ends at the next indexed record header, or at the stream end when no header follows.

**Need.** Files that vary the Extrude options and selected profile, to find which option writes each tail and what its fields name.

**Conflict.** None.

**Note.** None.

### DR-165. Composition of multiple Sketch-profile operands

**Question.** How does one Extrude profile group combine Sketch-profile operands from different Sketches when their nested selections use region and bounded-face forms?

**Known.** Each member uses the Sketch-profile frame in `f3d.md` §3.1. Each frame stores an asset UUID and a decimal Sketch entity suffix. Its nested selection can carry counted profile regions or a bounded-face recipe. The construction group preserves member order.

**Need.** A rule that combines the member selections and their Sketch frames into the complete extrusion profile.

**Conflict.** None.

**Note.** The decoder retains each profile operand in the native scope and emits `feature.extrude-profile-group-native` for each scope with multiple Sketch-profile operands.

### DR-257. Entity-selection serialization form

**Question.** What does the u32 serialization form after `u32 2` in a nested entity-selection member encode?

**Known.** `f3d.md` §3.1 "A nested entity-selection member has primary indexed record `N`." states that the field is `0` or `1`. The field layout after it is the same for both values. Every member in one Design stream stores the same value. Value `1` occurs in documents whose product version is an ASM 231 or 232 Windows build; documents from the same builds also store value `0`. Members of both values resolve their identities to Sketch curves that close Extrude profile regions and to Revolve axis lines.

**Need.** A pair of documents that differ only in the form value, and the Fusion setting or document generation that produces each value.

### DR-258. Second marked reference of the 87-byte scope tail

**Question.** What record does the marked reference at tail byte 30 of an 87-byte parameter-scope tail name, and does it change the operation of its scope?

**Known.** `f3d.md` §3.1 "A parameter scope is one logical indexed record" states that tail byte 30 of the 87-byte tail is `0x01` when it begins a second marked reference, and that the preceding history-state identity is then at tail offset 41. In `SplitFace` scopes the reference names record index 42 or 56, and no indexed header in the stream carries that index. These `SplitFace` scopes have the same splitting-tool and target groups as the 77- and 78-byte forms, and the projection does not read the reference.

**Need.** Controlled saves of one `SplitFace` before and after an edit that creates the reference, with the record that carries the referenced index.

### DR-263. Sketch Geometry member byte 34

**Question.** What sketch state does byte 34 of the sketch container's Geometry member encode?

**Known.** `f3d.md` §3.1 "The current sketch entity container has type GUID" gives the member layout. Byte 34 is `0` or `1` and lies between the u32 ordinal at member offset 30 and the visibility flag at offset 35. Its value is independent of the visibility flag, the ordinal collection, the placement orientation, and the planar or spatial form of the sketch geometry. Member type version 3 stores the same `0`/`1` byte between its ordinal and its visibility flag.

**Need.** Saves of one sketch that change one per-sketch setting at a time (Slice, 3D Sketch, Show Profile, Show Points, Show Dimensions, Show Constraints, Show Projected Geometries), each recording the value of byte 34.

**Note.** The decoder stores byte 34 in the native sketch visibility field `sketch_flag` and emits `sketch.geometry-flag-unprojected` when it is `1`.

### DR-264. Sketch geometry group fields

**Question.** What do byte `01` at offset 49 and the f32 values at offsets 62 and 66 of a sketch geometry group store, and which operations write a group?

**Known.** `f3d.md` §3.1 "A sketch geometry group has type GUID" gives the layout and the ownership joins. Every group stores `01` at offset 49 and f32 `1.0` at offsets 62 and 66. The sketch that owns a group is named by the reference table of a `MeshSection` scope. A controlled `Create Mesh Section Sketch` save from Fusion 2706.1.17, with a mesh body in a `Base Feature` and the XY construction plane, stores one group in the `86 + 11N` form with `01` at offset 49, f32 `1.0` at offsets 62 and 66, ordinal `1`, and 25 member references.

**Need.** Groups written by another operation, or groups with other values in these fields.

### DR-280. Body-recipe serialization form above one

**Question.** What tail layout does a body-recipe serialization form above `1` select?

**Known.** `f3d.md` §3.1 "A class-365 whole-body member starts after its indexed header" states that forms `0` and `1` store the standard tail layout. The decoder reads the standard tail layout for every form value, keeps the form bytes in the native operand, and emits `body-recipe.serialization-form-assumed` for each operand with a form above `1`.

**Need.** A document that stores a body-recipe operand with a form above `1`, and the tail layout of that operand.

**Conflict.** None.

**Note.** DR-257 records the serialization form of nested entity-selection members.

### DR-288. Sketch geometry group ordinal

**Question.** Does the u32 ordinal at offset 21 of a sketch geometry group equal the u32 ordinal at member offset 30 of its owning sketch container's Geometry member?

**Known.** `f3d.md` §3.1 "A sketch geometry group has type GUID" gives the group layout. The final reference of the group names its owning sketch container. `f3d.md` §3.1 "The current sketch entity container has type GUID" gives the positive ordinal at member offset 30 of that container's Geometry member.

**Need.** A group whose ordinal differs from the member ordinal of its owning sketch, or saves that renumber the sketches of one ordinal collection and record both values.

**Conflict.** None.

**Note.** The decoder keeps the group for each ordinal value and stores the ordinal in the `ordinal` field of the native `sketch_geometry_groups` record.

### DR-289. Sketch geometry group membership

**Question.** Which records does the member run of a sketch geometry group list, and does the counted member run of the owning sketch container name the group?

**Known.** `f3d.md` §3.1 "A sketch geometry group has type GUID" gives the counted member run in strictly increasing record-index order and the final owning-sketch reference. `f3d.md` §3.1 "When the final eleven bytes of a sketch point or curve primary record" assigns a point or curve whose direct owner backlink names a group to the sketch container that the group names.

**Need.** A group whose member run omits a record that backlinks to it, a group that lists a record with another backlink or a record of another type, and a sketch container whose counted member run omits its group.

**Conflict.** None.

**Note.** The decoder assigns a point or curve whose backlink names a group to the sketch that the group names, stores the group record index in `geometry_group`, and emits `sketch.geometry-group-member-unlisted` when the member run omits the record. The native `sketch_geometry_groups` record keeps the member run in `members` and the owning sketch reference in `sketch_reference`. A member reference that names a record without a decoded backlink to the group assigns no owner, and the decoder emits `sketch.geometry-group-member-unbacklinked`.

### DR-302. Word before the body-group reference of a Reverse Normal scope

**Question.** Is the u32 before the marked body-group reference of a `Reverse Normal` scope the count of the marked references that follow it?

**Known.** `f3d.md` §3.1 "A `Reverse Normal` scope reverses" gives u32 `1` at offset 21 of the compact prefix and offset 25 of the shifted prefix, followed by one marked reference to the body group. The body group carries every selected body. The decoder keeps a scope with another value as a native feature.

**Need.** A `Reverse Normal` scope with a value other than `1`, or a controlled save that selects bodies from more than one component.

**Conflict.** None.

**Note.** None.

### DR-303. GUID field of a Reverse Normal scope

**Question.** What does the LP-UTF16 GUID after the body-group reference of a `Reverse Normal` scope identify?

**Known.** `f3d.md` §3.1 "A `Reverse Normal` scope reverses" gives the null GUID at offset 36 of the compact prefix and offset 40 of the shifted prefix. The `CopyPaste` prefix stores the null GUID at the occurrence-key position when it has no removal operand. The decoder keeps a scope with another GUID as a native feature.

**Need.** A `Reverse Normal` scope with a GUID other than the null GUID, and the selected bodies and their component occurrence as the application shows them.

**Conflict.** None.

**Note.** None.
### DR-308. Canvas flag byte before the second boundary

**Question.** What does the flag byte before the second boundary segment of a Canvas geometry record select, and does a zero value change the image placement or display?

**Known.** `f3d.md` §3.1 "Canvas geometry has eight fixed prefix layouts." gives the byte position in each layout. The byte is zero in the legacy layout and zero or one in the other layouts. A modern Canvas with a zero byte stores both boundary segments, the plane reference, and the payload frame at the same offsets as a modern Canvas with a one byte.

**Need.** Controlled saves of one Canvas that toggle each Canvas display and selection option, compared byte for byte, to find the option that sets this byte.

**Conflict.** None.

**Note.** The decoder places a Canvas with a zero byte in a non-legacy layout by its boundary segments and payload frame, keeps the byte in the native `second_boundary_flag` field, and reports `appearance.reference-image-flag-assumed`.

## 2. External references

### DR-111. Component-path poses of nested occurrences

**Question.** How does a component-path record with more than two path elements join to a nested occurrence, and does its world pose supersede that occurrence's own placement?

**Known.** A two-element path of a cross-document element and a local element naming an occurrence-placement record stores the current world pose of that placed occurrence. A two-element path of a Component record and a local occurrence carrier of that component stores the current pose of a local occurrence in the root component. Longer paths continue through further cross-document and local elements, each with an instance discriminator, and also store a world transform. A path of a referenced design's root component and occurrence placement, then this document's placement of the external occurrence and some of its local owners, stores the world pose of the inner occurrence; when the records of one instance that list the most owners agree, they place it. A path of a referenced design's root component, then this document's placement of an external occurrence and one or more of its local owners, stores that external occurrence's world pose; it places an instance that no contained record places. An inline-qualified own path places its insertion independently of the contained carriers that identify child instances. A full-owner nested carrier in one public assembly stores origin `(75, -124.5, 450)` mm; its own-root carrier stores `(80, -125, 450)` mm, and its same-role contained-body carrier stores `(221.330802, -259.941433, 443.85)` mm. The class-301 pathless placement binds the nested carrier to its target and stores a local translation of `(0, 0, -0.5)` mm.

The first closing reference run identifies a direct contained child by its local insertion placement and cross-document target, in that order. Three nested-design carriers share their insertions with ordinary contained carriers and name distinct external child instances by this chain. Seven contained carriers in an inserted public design equal the product of the stored insertion placement and the direct source child pose. Their insertion's own current carrier states another insertion frame. The source composition preserves the child local pose under that current frame.

**Need.** Why an external occurrence's own two-element record, and a contained record that lists fewer owners, can keep an earlier pose while the records that list more owners give another; the join from each later path element to the placement record of the nested occurrence in its placed design, and the relation between that world pose and the nested occurrence's own placement and joints, and whether a two-local-element pose of an occurrence below another component is local to its container or in the world. The saved-state selector that relates a full-owner nested path to the own-root and same-role contained paths.

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

**Need.** The two bytes can be a grounding or visibility state of the occurrence. A controlled attributed occurrence must identify the marked source reference when its target has no type registration.

**Conflict.** None.

**Note.** The decoder reads an unregistered source as a component occurrence and reports `assembly.local-occurrence-source-unregistered`.

### DR-84. Occurrence identity lists of a typed path

**Question.** What do the occurrence identity lists after a typed occurrence path's identity GUIDs name?

**Known.** `f3d.md` "Assembly operands" gives the lists. In one assembly, the paths of four of its six joints carry the same single list of two keys: a decimal occurrence key and an occurrence GUID that name two different local occurrences. The paths of the other two joints, and a path continuation, carry no lists. That list is not the joint's own operand pair. In another assembly, lists hold three or four occurrence GUIDs, and the leading keys of a list can equal the leading occurrence GUIDs of the path that stores it.

**Need.** A writer must know when to emit a list and which occurrences it names.

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

**Question.** Which native relation binds a joint to an inserted-design occurrence that stores the identity and has no component-path carrier that states its pose, and in which frame does that joint define the solved pose?

**Known.** `f3d.md` §1.4 "The matrix is a proper rigid motion." defines stored identity placement and the component-path world-pose carriers. A joint names an occurrence directly through an occurrence container, or names an inserted design through an external operand container, leaving the neutral tie unresolved; each distinct external document then accounts for at most one unnamed identity occurrence. The decoder counts these occurrences, reports `assembly.joint-placed-occurrence-unsolved`, and places them at the identity with the joint unsolved. In one public assembly archive, eight root occurrences have carriers of a nested design, three of them store the identity placement, and the decoder places all eight from the carriers and removes them from the count. The root `As-built` scopes with a root operand path name none of those eight occurrence identities. A fourth identity occurrence has no carrier; the occurrences inside it keep one world pose in its insertion scope and in every captured position. A root display scene can store the solved pose of such an occurrence: for one fixed joint whose first operand path names an inserted identity occurrence and a child inside it, the scene's world matrices of all 31 child occurrences equal the pose that the Assemble connector equation gives, with the operand frame in the child's space. The negated angle gives no match.

**Need.** An inserted design whose occurrence stores the identity, has no component-path carrier, and is placed by a joint, with the solved pose known, must name the binding relation (direct occurrence reference or external-document container) and the frame of the solved pose, before the decoder can place these occurrences from their joints.

**Note.** DR-113 covers solved placement without a bound assembly operand generally; this joint-to-occurrence binding is a separate question.

### DR-119. Entity-cluster component-ownership fallback

**Question.** When no admitted Body-container membership names a solved body, which entity cluster selects its component owner, and when does that selection disagree with the authored ownership?

**Known.** `f3d.md` §3.1 "The paired untyped Body-container membership" defines the admitted membership join. A solved body binding in the current model without that membership takes entity-cluster component ownership, and the decoder reports `assembly.component-body-owner-fallback`. Owner-graph cycles and expansion-limit drops leave occurrences unplaced under `assembly.local-occurrence-owner-cycle` and `assembly.local-occurrence-expansion-limit`.

**Need.** Controlled saves that delete or omit Body-container memberships while holding entity clusters fixed must show which cluster the fallback selects and whether the selected owner matches the authored component.

**Note.** DR-75 defines the local occurrence carrier GUID; the validity of this fallback is a separate question.

### XR-05. F3Z member cycle, absence, and decode-failure paths

**Question.** Do the cycle, absence, and decode-failure losses exhaust the unresolvable member states of a multi-document merge?

**Known.** `f3d.md` §1.5 "XREF reference resolution is member-local and recursive." defines member-local recursive resolution. A member path that recurs in the active chain reports `xref.cycle` and stays unresolved. A member absent from the archive reports `xref.member-missing`. A present member that is not an F3D document member, that cannot be scanned, or that fails to decode reports `xref.member-undecoded`. Unscannable members are marked unreadable at scan time and skipped at merge time with that scan-time mark as their record. Resource-limit failures return an error.

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

### DR-324. Cage order of a Form bound through body records

**Question.** In which order does a Form scope that several Form body records name hold its cages?

**Known.** `f3d.md` §1.1.1 "**Form body-record cage join.**" binds each cage to its owning Form scope through one Form body record. The scope's reference table holds a result-owner record and a Form selection group, and its selection members are tracking records. The decoder orders the cages by ascending body-record index and reports `feature.form-cage-order-assumed` for each such Form with two or more cages.

**Need.** Controlled saves of one Form session that creates two bodies in one order and then changes the first body, to compare the body order of the session with the body-record index order.

**Conflict.** None.

**Note.** None.

### DR-325. Empty Form selection group

**Question.** What does a zero member count in a Form selection group mean for the cages committed by its scope?

**Known.** A Form scope can have a selection group with zero members and no Form body record that names the scope. The decoder projects an empty cage list for that combination and reports `feature.form-empty-cages-assumed`.

**Need.** Controlled saves of empty and edited Form sessions that compare the selection group, body records, and committed cages.

**Conflict.** None.

**Note.** None.

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

**Question.** Which reference assignments and orientation fields define a first-side face termination with a second-side distance, and what do target options zero and two select?

**Known.** The current seven-slot prologue can carry direction two, first-side extent two, and a payload second-side extent one. Target option zero also occurs with a payload second-side extent one or two. Target option two occurs with a payload second-side extent two. The final zero word before the scope reference count does not identify the second-side termination. The decoder reads target options zero and two with the side terminations of target option one and reports `feature.extrude-target-option-assumed`.

**Need.** Controlled face-then-distance changes with both direction states and independent side tapers.

**Conflict.** None.

**Note.** The scope and ordered references remain native.

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

**Question.** Does a form-four Align constructor matrix state the complete relative motion of its selected body group? What do its leading scalar, nullable reference slots, unsigned codes, auxiliary scope references, and omitted-matrix marker control? Which alignment geometry do the role-`0x0000000500000000` construction groups of the scope select? What do the codes and marked words of the eight-word constructor control?

**Known.** The Align prefix contains two references, an unsigned code, and a zero GUID. The references join to the ordered scope table. The compact and extended forms differ by one u32 word of value 1. The first reference selects the constructor. A version-two rigid-transform constructor stores source form four, a leading scalar, six nullable references, two unsigned codes, and a matrix-presence marker. Its row-major matrix has an orthonormal basis, a homogeneous final row, and translation in centimetres. The second prefix reference names the role-`0x0000000400000000` whole-body construction group at scope-reference ordinal zero. A one matrix-presence marker omits sixteen scalars and retains the common tail. The eight-word constructor form retains its marked words, codes, matrix scalars, separate scalar, and two scope back-references.

**Need.** Controlled Align exports that change object selection, source and target frames, reversal, and angle independently. F22b supplies a body-alignment recipe.

**Conflict.** The stored matrix can describe a solved frame or a relative motion.

**Note.** Apply the form-four matrix to the selected body group with `feature.align-motion-assumed`. Treat an omitted matrix as identity with `feature.align-identity-assumed`. A scope with zero or several matching body groups projects an unresolved body selection with `feature.align-operands-unresolved`. Preserve the constructor fields and scope references natively. An unsupported motion reports `feature.align-motion-unprojected`.

### DR-144. Flag bytes of the discriminated parameter expression trailer

**Question.** What do the first four of the five flag bytes in the nine-byte expression trailer of a discriminated parameter frame state?

**Known.** The trailer is four zero bytes and five flag bytes, each `0` or `1`. The last flag byte is `1` exactly for a document parameter. Owned parameters set the first, second, or third flag byte, alone or the second and third together. Parameters with a null owner reference set the fourth flag byte, alone or with the first or third. Every frame with a set flag byte has the same remaining grammar as a frame with zero flag bytes.

**Need.** Files that change one parameter property at a time, such as favorite state, comment, or the deletion of the owning dimension, and save the document after each change.

**Conflict.** None.

**Note.** The decoder admits the flag bytes and does not project them.

### DR-222. `MoveFace` and `OffsetFaces` member recipes that resolve no face

**Question.** Which face does a `MoveFace` or `OffsetFaces` bounded-face member select when the `DeleteFace` bounded-face rule resolves no unique face for its recipe?

**Known.** A member selects the face that its bounded-face recipe resolves in the input state (`f3d.md`, "The members of a `MoveFace` or `OffsetFaces` group also state the selection"). The rule resolves nothing for a member with a node whose structure matches no recipe-structure form, a positive program header that differs from the node count, or node ordinals beyond the prefix operands.

**Need.** The grammar of these node forms.

**Conflict.** None.

**Note.** Keep the group as the native selection.

### DR-223. `Shell` removed faces that the transition leaves open

**Question.** Which faces does a `Shell` removed-face group select when a face that its transition updates or deletes has no plane or cylinder row, when the changed faces without a wall outnumber the group's distinct members, or when the shell stores unequal or skin walls?

**Known.** The specification states the removed faces from the walls of the transition when every changed face has a row to compare. A face on a cone, sphere, torus, or spline surface has no row. A kept face whose wall merges with another face, or a removed face with an unrelated face at the wall offset, changes the count. Each group member is a bounded-face recipe.

**Need.** The mapping from a bounded-face member recipe to the face it selects, or a wall comparison for the other surface forms.

**Conflict.** None.

**Note.** Keep the group as the native selection.

### DR-126. Auxiliary references of a Move scope

**Question.** How do the ordered references beyond the body group, its members, and the transform record affect the motion of a `Move` scope?

**Known.** `f3d.md` §3.1 defines the marked transform in the marker-zero, marker-one, and inline-typed prologues. The decoder applies the stored matrix to the group's whole bodies and records `feature.move-scope-references-assumed` with the auxiliary reference indices. A form-2 transform also records `feature.move-transform-references-assumed` for its four construction references (DR-375).

**Need.** Controlled saves that change the pivot, source and target constructions, and auxiliary selections independently while preserving the stored matrix.

**Conflict.** None.

**Note.** The auxiliary references are assumed to leave the stored motion unchanged.

### DR-306. Marked Move transform records outside the rigid-transform rules

**Question.** What do the leading scalar, nullable reference slots, unsigned prefix codes, source forms four and six, and omitted-matrix marker of a rigid-transform record control, and what does a transform record that the marked reference of a `Move` or `Modify` scope selects store when its class type is not the rigid-transform type, its frame is not a length of its type version, or its form word is not the form word of its frame length, and what frame does a version-3 record with an omitted matrix have?

**Known.** `f3d.md` §3.1 "`Move` scopes with a fixed transform frame" gives the rigid-transform type, the form word of each frame length of type versions 2 and 3, and the marked reference that selects the transform record. A marked reference selects a 256-byte record with form word `5`, the zero run, and a rigid matrix at offset 48. The decoder keeps the matrix of each marked record with a finite leading scalar, six nullable reference slots, two unsigned code words, and source form `0`, `1`, `2`, `4`, `5`, or `6`. Zero leading scalar and first two null slots reduce this prefix to the four-slot form. `Move` and `Modify` scopes record `feature.move-transform-assumed` with each fixed-frame rule that the record breaks. A nonzero leading scalar, present leading references, partially populated remaining reference run, or nonzero code words project with `feature.move-transform-prefix-assumed`. Forms four and six project with `feature.move-transform-mode-assumed`. These fields are assumed to leave the stored matrix motion unchanged. The native operation retains the scalar, references, inline type GUIDs, code words, and matrix-omission flag. A one matrix-presence marker omits 128 matrix bytes and preserves the common tail. Form four, and form five with zero scalar, null references and zero codes, admit identity from a 77- or 78-byte tail with `feature.move-transform-identity-assumed`.

**Need.** A controlled `Move` of a body that a copy creates, recording the class type, frame length, and form word of its marked record, which can carry class type `6CCF41D5-40BE-48ED-A834-18F3EAED6C57`; and `Move` saves that set a nonzero leading scalar or code word.

**Conflict.** None.

**Note.** None.

### DR-400. Plane-frame record of a plane selection

**Question.** Which fields of the record that an entity-selection member's trailing reference names state the selected plane's frame, and does every plane selection store one?

**Known.** The trailing reference of an entity-selection member that selects a plane names a record with type GUID `CC54ECAD-6DE8-4DE1-803D-BBE0E751975B`. The record stores the plane origin in centimetres and the unit u and v axes, each as three f64 values. A selection of the XY, XZ, or YZ origin plane stores the canonical origin-plane frame, and a `WorkPlane` selection stores the columns of the `WorkPlane` matrix. The decoder takes an origin plane's frame from the canonical table by ordinal and does not read or compare the record.

**Need.** Saves with an angled `WorkPlane`, a `WorkPlane` moved after the selecting feature, and an origin plane of a non-root component, to type the remaining bytes of the record and its refresh rule.

**Conflict.** None.

**Note.** Draft and Revolve plane targets use the same selection member.

### DR-375. References of a form-2 rigid-transform record

**Question.** What do the four marked references of a 293-byte form-2 rigid-transform record name, and do they change the motion that its matrix stores?

**Known.** `f3d.md` §3.1 "`Move` scopes with a fixed transform frame" gives the 293-byte frame: four marked references, form word `2`, and a rigid matrix at offset 88. The inline-typed spelling has four 51-byte references and its matrix at offset 248. The decoder projects the matrix as the motion of the `Move`, `Modify`, or `MoveFace` scope that selects the record, and records `feature.move-transform-references-assumed`.

**Need.** Controlled saves of one `Move` that use point-to-point and other reference-based motion types, each recording the records that the four references name.

**Conflict.** None.

**Note.** None.

### DR-374. `Modify` body groups that the prologue does not name

**Question.** What do the first `N` body groups of a `Modify` scope's ordered reference table select, and how do they differ from the `N` groups that the prologue names?

**Known.** `f3d.md` §3.1 "A `Modify` scope moves whole bodies by one rigid transform" gives the prologue, the `2N` single-member body groups, and the last `N` groups that the prologue names. The decoder projects a `MoveBody` of the bodies of the named groups with the motion of the transform record, and records `feature.modify-unlisted-groups-assumed` with the unnamed groups. It records `feature.modify-table-assumed` with each table position of the `2N`-group run that is not a single-member body group followed by its member, and `feature.modify-state-assumed` when the scope commits a current history state. A `Modify` scope that has no complete named body group stays native with `feature.modify-unprojected`.

**Need.** Controlled saves of one `Modify` that move one body and then two bodies, and a save that selects a different body after the first selection, each recording the member records of all `2N` groups.

**Conflict.** None.

**Note.** None.

### DR-224. `Fillet` edges that the transition leaves open

**Question.** Which edges does a constant-radius `Fillet` group select when its transition inserts a blend without a cylinder or torus row, a blend with no replaced edge or with two, two groups with one radius, or more replaced edges than the group has distinct members? Can a rounded edge keep its slot?

**Known.** The specification states the edges from the blend faces of the transition when every new carrier is a cylinder or torus blend. A sphere, spline, or variable blend has no spine row. A tangent chain rounds more edges than the group has members. A historical partial selection can name an edge that the transition keeps. The covering rule takes every other shared edge that the transition deletes as consumed by the blend. In a control whose fillet passes a parallel edge 1.5 mm from the rounded corner between planes, the supports share both edges, the rounded edge covers the extent, and the consumed edge covers half of it.

**Need.** Controls that fillet one edge of a tangent chain, a corner of three rounded edges, and an edge between curved faces, controls that show whether a selected edge can keep its slot, and controls whose blend consumes a parallel edge between curved supports or along the whole blend.

**Conflict.** None.

**Note.** Keep the operand proofs for these groups.

The bounded-face Chamfer 4 members in `Parametric_PTFE_Umbilcal_guide.f3z` are tracked in DR-481.

### DR-390. Fillet set flag of the chordal and compact forms, marker bytes, and compact flag run

**Question.** Do the flag bytes of the chordal and compact fillet set forms encode tangent chain, do those forms store curvature continuity, and what do the set marker byte, the `00 01` tail of the current flag run, and the compact flag-run form encode?

**Known.** `f3d.md` §3.1 "A parameter-owned `Fillet` scope names its fillet sets in a prefix" gives their positions and admitted values. The decoder stores the flag of every set form as the set's `tangent_chain`, reads curvature continuity in the constant form only, and stores `rolling_ball_corner = true` for the compact flag run, whose first byte is `01`.

**Need.** Chordal and compact Fillet saves with tangent chain off, with curvature continuity, and with setback corners.

**Conflict.** None.

### DR-460. Chamfer set flag of the other forms, entry count, fixed byte, and auxiliary record

**Question.** Do the flag bytes of the two-distance, distance-angle, and compact chamfer set forms encode tangent chain, does the entry run count the selected edges or the edges after tangent propagation, and what do the zero byte that ends each entry, the fixed `u8 1`, the entry-run marker forms, and the auxiliary record encode?

**Known.** `f3d.md` §3.1 "A parameter-owned `Chamfer` scope names its chamfer sets in a prefix" gives their positions and admitted values. The decoder stores the flag of every set form as the set's `tangent_chain`, the marker as the list's `form`, the auxiliary record index as the set's `auxiliary_record_index`, and the entries per set as the set's `entry_count`. In every decoded save the selected edges have no tangent neighbour, and `E` equals the number of selected edges.

**Need.** Controlled saves of two-distance, distance-angle, and compact chamfers with tangent chain off, and a chamfer on one edge with tangent neighbours.

**Conflict.** None.

### DR-243. Face of an Extents To Hole

**Question.** Which face does the member of an Extents To Hole's role-`0x0000000500000000` group name?

**Known.** The member is a `face` recipe operand that follows the group in the scope reference table. In the controls, its candidate lanes name the bottom face of the plate, which is where the hole ends. The start-face `bounded_face` operands of the same scopes name that bottom face too, although the hole starts on the top face.

**Need.** Controls that end the hole on a side face, on a face of another body, and with an offset from the face.

**Conflict.** None.

**Note.** The decoder projects the one face that the member resolves as the terminating face and reports `feature.hole-extent-face-assumed`. A member that resolves no face or several faces keeps the group as the native face selection.

### DR-254. Reference edges of a Hole's placement distances

**Question.** Which role-`0x0000002100000000` group does each of `RefDim1` and `RefDim2` measure from, and does a distance carry a sign?

**Known.** A Hole placed by reference-edge distances stores `RefDim1` and optionally `RefDim2`. One distance has one role-`0x0000002100000000` group, and two distances have two such groups. Its carrier point stores the resulting position.

**Need.** Controls with two different distances, with the edges selected in both orders, and with a hole on the other side of an edge.

**Conflict.** None.

**Note.** The Hole projects from its carrier point and reports `feature.hole-reference-dimensions-native`. Unproved distance-to-group associations retain the scalar dimensions and report `feature.hole-parameters-independent-assumed`.

### DR-470. Independent parameters of a partial Hole

**Question.** Which Hole parameters remain active when the extent controls, parameter associations, or entry-treatment dimensions leave the complete construction unresolved, and which standard sizing controls define a neutral clearance or thread specification?

**Known.** Extent control `1` can coexist with a `HoleDepth` owner and an absent saved-depth property. The parameter owners retain the diameter, tip, treatment and reference-distance expressions and evaluated values. The sizing block retains four ordered names for standard, fit, size and fastener, followed by the entry-type word for simple, counterbore or countersink form.

**Need.** Controlled non-distance Holes that vary the stored depth independently of the target, partial and competing treatment parameter sets, and standard sizing controls that establish modeled, cosmetic, handedness and depth settings.

**Conflict.** None.

**Note.** The decoder retains independently admitted dimensions, parameter expressions and extent metadata on a neutral Hole. A readable entry-type word selects the active form; unmatched treatment parameters remain attached. An unresolved extent, entry-type or parameter association reports `feature.hole-parameters-independent-assumed`; an incomplete or conflicting shape reports `feature.hole-shape-partial-assumed`. Native sizing names remain source properties and report `feature.hole-standard-sizing-assumed`.

### DR-471. Hole drilling direction with an unavailable control

**Question.** Which drilling direction applies when a Hole retains its carrier and entry points while its direction control is unavailable?

**Known.** The carrier stores a model-space normal, and its admitted point forms give the entry positions. Direction controls `0` and `1` orient the drill against and along that normal respectively. Legacy extent forms can retain these placements while omitting the direction control.

**Need.** Controlled legacy Holes with opposite drill directions at the same entry points and support plane.

**Conflict.** None.

**Note.** The decoder retains each entry position and carrier normal as an unoriented Hole axis and reports `feature.hole-carrier-axis-assumed`.

### DR-124. Partial-curve operands in profile selections

**Question.** How do the three u32 operands of a typed profile curve entry and the two nonzero incidence words of a counted profile member select a partial curve, and which curve's direction sets the direction flag of the members of a coincident edge?

**Known.** The identity record stores region groups. Each region has counted boundary loops with exterior flags. Each curve entry stores a primary identity, a secondary identity, and three u32 operands. Counted profile members store a primary curve identity, a direction flag, and two nonzero incidence words. A complete neutral loop match, a unique atomic arrangement boundary program, or unique directed boundary rings resolve the selected region. The decoder builds the member-ring arrangement from lines, circles, circular arcs, bounded ellipses, and nonperiodic NURBS. A bounded ellipse leaves the arrangement undefined, because its edges have no certified pieces. The nodes are the end points of these curves and the intersection points of every two lines, circular arcs, or circles that are not collinear line segments. A node splits a line, circular arc, or circle that passes through it; a NURBS splits only at its end points. Edges leave a node in the counterclockwise order of their departure tangents, each turned by half its exact curvature over the linear tolerance. The tangents are exact for lines, circular arcs, and clamped NURBS ends with positive weights. Equal tangents order by signed curvature. A clamped NURBS end has only the sign of its curvature, so two equal signs leave the arrangement undefined, as do two equal exact curvatures. Another edge leaves along the first chord of its polyline, and a node with such a departure and more than two edges leaves the arrangement undefined. The selection stays native with `feature.profile-selection-native` when two edges meet away from their common nodes, as where a NURBS crosses another curve, or when one curve runs twice along one edge or in both directions along one fragment. The exact lines and circular arcs and the certified NURBS tubes of a face boundary, each in the direction of its walk, decide whether a point lies inside the face. A face holds a component when it holds the first node of the component that lies farther than the linear tolerance from every boundary piece. An island lies in each face that holds its component, and the innermost of these faces holds it; one face lies inside another when the other face holds its component. When no node of the component clears the pieces of a face, or no face that holds the island is the innermost, that face and the faces that hold the island have unknown islands. When the island has no unique outer walk, the innermost face and the island's own faces have unknown islands. A selection of a face with unknown islands stays native. A projected point within the linear tolerance of a NURBS tube of a face boundary takes the side of the face polyline, and the selection reports `feature.profile-polyline-side-assumed`. For member lists, projected points, curve programs, and self-crossing loops in the arrangement of every non-construction curve, a non-construction full ellipse or periodic NURBS without a negative weight has a box that holds it: the exact axis-aligned box of the ellipse, or the box of the control points of the NURBS. The box of a face holds its lines and circular arcs exactly and its NURBS edges within their certified tube bounds. The curve lies outside a face when its box misses the face box by more than the linear tolerance, or when its box, widened by the linear tolerance, meets and holds no boundary piece of the face and the box centre lies outside the face. When the widened box meets and holds no boundary piece of any face, the curve lies in the region of the smallest face that holds the box centre, and that face then bounds no arrangement region. For every other face that holds or can hold such a curve, and for every face and every other non-construction curve outside these families, the decoder takes the face without an island of that curve and reports `feature.profile-island-absence-assumed`. A selection whose profile takes no sketch regions reports neither loss. For a walk with an edge of coincident curves, the decoder accepts the edge when one distinct member that names one of its curves has that curve's traversal direction, and it checks no flag of the edge's other members. This filter can choose one of several walks with the list's curve identities and multiplicities, and it can reject the only such walk. The decoder reports `feature.profile-coincident-direction-assumed` for each bound selection in which a walk with such an edge has a list's curve identities and multiplicities. The member-ring search charges the arrangement work budget, and an exhausted budget leaves the selection native with `feature.profile-arrangement-capped`. The decoder matches a boundary program against the arrangement of the curves of the Sketch's profile loops, or of every non-construction curve when the Sketch has no profile loop. A typed region whose loops match no curve program is read through the same directed rings: each stored loop is a ring, its exterior flag makes it the outer ring or a hole, and the first operand of each entry is read as its traversal flag, with `1` against the curve's direction. Such a selection reports `feature.profile-loop-flags-assumed`. The other two operands are not read. Public `Parametric_Wago_Mount v4.f3d` Extrude 3 scope 4302 has profile operand 4319 and counted selection 4322 at offset 112725. Its ordered members are 150, 147, 146, 145, 214, 210, 208, and 148; curve 150 carries incidence values `(2, 3)`. Sketch 187 has zero neutral profiles and contains partially overlapping lines. Public dual and single rolling-spool mounts have Extrude 2 scope 418, profile operand 429, and selection 432. Their selection headers are at offsets 105061 and 89266. Both list curves 140, 122, 114, 133, 124, and 117. Curves 122 and 124 carry `(2, 3)`; curves 114 and 117 carry `(3, 5)`. The short line 122 runs opposite the coincident part of long line 114, and both members have direction flag one. Curves 124 and 117 have the same relation on the opposite boundary. Each sketch has two complete neutral profiles. The decoder retains these three feature selections. All offsets refer to `FusionAssetName[Active]/Design1/BulkStream.dat`.

**Need.** Controlled selections of adjacent regions with one shared curve, and of intersecting curves with distinct selected regions, to bind each operand to curve orientation and trim bounds. Selections with the same curve-identity multiset must vary the selected portions independently. Controlled selections of one region whose boundary edge is two coincident curves drawn in opposite directions, with each curve created first in turn, bind the direction flag of a coincident edge.

**Conflict.** None.

**Note.** A repeated identity within one source loop does not prove a complete neutral loop. A program without one unique atomic arrangement match retains its native identity and loss.

### DR-125. Counted face-target recipe without a unique historical face

**Question.** Which counted bounded-face recipe clauses select a termination face when persistent references retain several preceding support faces?

**Known.** A termination group identifies the face operand. The operand retains its counted recipe nodes, ordered prefix references, and state-local support contexts. A unique preceding face or convergent support context supplies a historical selection.

**Need.** Controlled face-target selections that vary the selected planar face while the body topology and other prefix references remain fixed.

**Conflict.** None.

**Note.** Candidate membership, planar support, and absence of changed faces do not by themselves identify a selected face.

The compact Sketch-point termination of Extrude 4 in `M3 Parametric SHCS.f3d` is tracked in DR-480.

### DR-122. Derived Base Feature source and frame

**Question.** Which field of a `Derived Base Feature` scope identifies the derived source body, and which field stores its local frame or mirror plane?

**Known.** The 711-byte frame stores nine typed-reference elements, a zero GUID, the kind, the ordinal, and an f64 of unknown meaning. The second body element of the 711-byte frame stores the Design body entity suffix of the result body. Every reference in the scope is a Design record of the same stream. The frame stores no matrix. The `DerivedBaseFeature` forms (707 and 491 bytes) store the same elements and roles, a part with a mirror label places its occurrence with an identity rotation, and no field in either form stores a plane or a matrix. The 761-byte frame has one additional typed-reference element with a role GUID that no 711-byte frame stores. Its record index is lower than the scope record index.

**Need.** A small design with two derived scopes and two source documents. Each source document must contain two geometrically equal bodies. Save one pair that changes one scope's source document. Save another pair that selects the other source body. Reverse the dependency creation order in a third pair. Keep result body identities and geometry fixed in these pairs. Save separate translation, rotation, and mirror changes. Keep the source body identity and local geometry fixed for these frame changes. Include a source component with one Sketch and zero bodies, then add one body while keeping its component identity. Include the deriving archive and both source documents at each save. Trace each scope through its local references and GUID fields to one DERIVED row, source body, and stored frame. Compare the f64 and the additional 761-byte element across these controls.

**Conflict.** None.

**Note.** A scope without proven result bodies remains native with `feature.derived-base-unresolved`. A scope with proven result bodies is kept as a stored snapshot with the same code until source and frame have a complete binding. The public Voron Trident `z_mounts_front.f3d` has nine such scopes. Component context bindings identify their result owners. The source-body and source-frame joins require the derivation references independently of those owner bindings.

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

### DR-146. Later ordinal in a fixed or leading-flag identity occurrence

**Question.** What placement does a fixed untyped or leading-flag occurrence with identity flag `1` and ordinal greater than one select?

**Known.** Two standard 229-byte frames store ordinal `2` and identity flag `1`. Attributed identity frames retain later ordinals. The leading-flag frame stores the same identity flag and one-based ordinal.

**Need.** Independent files that vary the occurrence ordinal and placement separately.

**Conflict.** None.

**Note.** Emit `assembly.local-occurrence-identity-ordinal-unresolved` and keep the occurrence out of neutral product structure.

### DR-142. Thread construction option bytes

**Question.** What does the second byte of the Thread construction marker select? In the forms other than the extended standard form, what do the first marker byte, the pitch marker, and the zero bytes before the trailer `01` select?

**Known.** The construction marker first byte is `0` or `1` and its second byte is `1` or `2`. The pitch marker is `0` or `1`. In the extended standard form, the first marker byte is the full-length flag, the pitch-marker byte is the modeled flag, and the second marker byte is `1`. The other admitted forms fix each combination they accept. In the standard-prefix compact-marker form, the run before the trailer `01` is two or three zero bytes. Every byte in that run is zero in each Thread scope that carries it.

**Need.** Thread scopes of the direct, owner-marked, and extended compact forms saved with one dialog option changed at a time.

**Conflict.** None.

**Note.** A scope whose marker combination no admitted form accepts stays an unresolved Thread operation.

### DR-152. Mirror selection run with role 0x10

**Question.** What do the members of a Mirror scope's role-`0x10` group select?

**Known.** `f3d.md` §3.1 "A Design Mirror scope" lists the seed and plane groups. Mirror scopes in the public native-assembly drive-unit design also store a counted role-`0x10` group whose members are class-256 persistent selection records. The same scopes store a role-`0x5` plane group with one bounded-face recipe member. When a Mirror scope has no non-empty role-`0x4` or role-`0x8` group, the decoder uses the role-`0x10` group at scope-reference ordinal one as the seed group of the mirror construction. The members of that group supply face seeds when each decodes as a bounded-face operand. A plane entity selection can have a 29-byte compact identity record whose primary identity is not an origin entity. When the scope has no plane reference and the face candidates of the primary and persistent identities give no plane, the decoder reads the plane from the scope in the same stream at the compact identity plus one. That scope must be a WorkPlane with a finite frame. Each candidate face must be absent from the preceding history state, or lie on one surface with an analytic non-plane definition in that state. A candidate face that gives a plane, a candidate face that does not meet these conditions, or a history that holds the identity and supplies no candidate face voids this reading. The decoder reports `feature.mirror-plane-work-plane-assumed` with each candidate face and its defect.

**Need.** Mirror features with controlled changes to the selected objects, to find which seed kind the group carries and how it relates to the seed group. Mirror features whose plane is a construction plane, saved with the plane selection in each identity form, to show whether the compact identity plus one always names the selected WorkPlane.

**Conflict.** None.

**Note.** None.

### DR-153. Instance discriminator of a component path

**Question.** How does the u32 instance discriminator of a local component path select one expanded instance when several instances of the target have the listed containing carriers?

**Known.** No two component-path records of a Design stream share a record and a discriminator. A path that lists the full chain of containing carriers names one instance. The initial element list can name several instances below its listed carriers. The first closing reference run carries the full ordered occurrence chain defined in `f3d.md` §1.4. In one public assembly, 105 of 1241 initial element lists name several instances and two reach no instance. For one target with 16 expanded instances and 16 records, the target discriminators are 0 to 15 once each, and the order of the target discriminators equals the order of the Component element discriminators. No tested order of the expanded instances reproduces the target discriminators: ordering by the records from the outermost carrier inward matches for 46 of 77 targets.

**Need.** Designs with a repeated holder of known instance order, made by copy and by pattern, with the component-path records before and after reordering the instances.

**Conflict.** None.

**Note.** A path whose initial elements and local closing occurrence chain name one instance supplies its stored world pose. A path that still names more than one instance is placed only when the layout-consistency inference in `docs/format-support.md` selects exactly one assignment; it is then reported as `assembly.local-occurrence-path-pose-inferred`. Otherwise it is reported as `assembly.local-occurrence-path-pose-unresolved`.

### DR-150. History namespace of copied feature scopes

**Question.** Which native relation binds a parameter scope to its ASM history when several histories contain the same endpoint pair and the scope has no unique body-map or whole-body recipe binding?

**Known.** Current and preceding state identities are local to one ASM history. Independent history graphs can contain the same endpoint pair and complete predecessor chain. A result body-map carrier or whole-body recipe candidate faces can bind a scope to one BREP basename. Copied feature groups can retain the same endpoint pairs in separate histories. Each component's bodies live in their own BREP blob, and a history's id names its blob. When a scope has no such binding, the decoder reads the feature's history properties from the one history of the BREP blob that holds every body the feature outputs, or that a Base Feature resolves, when exactly one history of that blob holds the pair, and reports `history.output-blob-history-inferred`.

**Need.** The native reference chain from each copied scope to its history or BREP basename. Files that change the copied scope's source component while they retain its endpoint pair.

**Conflict.** None.

**Note.** An ambiguous interval retains its native scope, ordered references, state identities, and candidate histories with `feature.output-state-chain-unbound`.

### DR-151. Result ownership of copied feature scopes

**Question.** How does a copied feature scope relate its construction component to the product definition of its result bodies?

**Known.** A scope has a component owner. Its ASM transition interval identifies affected body slots in one BREP namespace. An affected active body can belong to a different product definition from the scope's component owner.

**Need.** A two-component design with a copied feature whose history interval touches bodies owned by both components. Use equal geometry on the two bodies. Save one pair that changes the copied feature's component and preserves its construction references, body identities, local body geometry, and component placements. Save separate pairs that change one body's component while the feature component and construction references stay fixed. Start with identity component placements. Then translate and rotate one component in separate saves while local body geometry and ownership stay fixed. Trace the scope owner, active body-map pairs, and local frame of each result body through each pair.

**Conflict.** None.

**Note.** Result body links outside the scope's product definition remain native with `scope.output-owner-mismatch`.

### DR-162. Component paths with two nested occurrence placements

**Question.** What pose does a component-path record state when its path holds a root component, a cross-document occurrence placement of the same role, this document's occurrence placement, and a second cross-document occurrence placement of the external occurrence's role?

**Known.** Such records store a rigid matrix after the path, like the records `f3d.md` §1.4 defines. The local element can state the occurrence-placement type inline. A public assembly archive stores 38 of them for two inserted designs. The decoder reads none of them.

**Need.** Records of a design that nests a design two levels deep with one occurrence moved at each level, to find whether the matrix is the world pose of the innermost occurrence or of the middle one.

**Conflict.** None.

**Note.** None.

### DR-164. Primary edge-recipe references with several face fragments

**Question.** Which selector rule selects the edge when several edges remain after the primary-reference lane unions, distinct-support-face constraint, and recipe-boundary constraint? Which face does a lane select when its persistent names bind to disjoint face sets? Does the singleton deletion proof apply to a one-side Chamfer recipe?

**Known.** The first side carries one-based prefix-reference ordinals. A prefix reference can bind to several preceding faces through persistent token and Design-reference tags. Each first-side reference lane contributes the union of its live face-fragment loop edges. The lane intersection requires distinct supporting faces. The recipe-boundary constraint selects a unique edge. A standard two-side Chamfer or Fillet recipe has transition proof for a singleton lane intersection when the owning transition deletes that edge and the persistent names within each lane have a common face. The public Trident front Z mounts have 69 singleton selections outside the candidate-face boundaries: 56 Chamfer and 13 Fillet operands, including one fragment-lane operand of each kind. Every selected edge is deleted, and its supporting faces change in the owning transition. Independent controls include the Micron beefy handle and B motor tail mount, Mini AfterSherpa Orbiter V2 test, and V6 Zodiac BMO. Stored recipe programs, face-loop boundaries, edge revisions, and state transitions prove the same reading in these controls. The Trident rear Z carriage has one one-side Chamfer operand at Design offset 1995667, scope record 115756. Its first side names faces 1602 and 2154, whose common edge is slot 1718 in state 353. State 354 deletes that edge and changes both faces. Its 37-word program starts at Design offset 1996254. The side header is `0`, its scalars are `[1, 2, 1]`, and its two topology entries have boundary counts `2` and `7`. The recipe candidate face is 1872. This one-side reading retains `history.edge-selection-inferred`. A disjoint-name lane still uses a deleted-edge restriction as an assumption.

**Need.** The selector rule for several remaining edges, the face-binding rule for disjoint persistent names, and an independent one-side Chamfer design that distinguishes the selected edge from collateral deletion. Controlled recipe changes that preserve the prefix references and vary the selected fragment must distinguish the candidate rules.

**Conflict.** None.

**Note.** An unresolved member retains the native group. A singleton without the deletion proof and a disjoint-name reading report `history.edge-selection-inferred`. A non-unique or incomplete reference proof reports `history.edge-midpoints-unresolved` when the feature requires edge samples.

### DR-170. Unresolved DeleteFace operand bindings

**Question.** When the transition deletes more faces than the group has members, which deleted faces does the group select? Which native rules bind DeleteFace operands with a one-clause node, an empty live prefix-face lane, repeated edge-incidence matches, or a prefix-fragment face fan that fails the live topology match?

**Known.** `f3d.md` §3.1 "The scope's preceding and current state identities delimit" settles a group whose member count equals the deleted-face count. Healing can delete a further face, so a larger deleted set does not separate selected faces from healed faces. Such a group resolves only through the complete-boundary rule in `f3d.md` §3.1. Some of its operands have ten opener-framed nodes whose second prelude scalar is `1`, and the two-clause structure parser retains their raw programs. Some have a referenced prefix ordinal with an empty face lane in the preceding topology. Some have multiple boundary-edge matches after incident-pair and cyclic endpoint-fan matching. Some have prefix lanes that repeat a face fragment while the boundary endpoint fans require another face.

**Need.** The edge that a one-clause node binds, and the role of a segment that opens with `[-1, -1, 1]`; the binding of an empty prefix lane to a preceding face; the directional or topology-entry rule that distinguishes repeated edge matches; and the ordinal and fragment rule for the mismatched prefix fan. Independent selections that vary these controls must distinguish the possible bindings.

**Note.** Such a group retains its native identity and reports `feature.face-selection-native`.

### DR-262. Faces of transition-inferred Extrude face profiles

**Question.** Which preceding faces does an Extrude face profile with unresolved members select, and which of them does each unresolved member select?

**Known.** The unresolved members have recipe nodes outside the two-clause structure, or positive prefix references whose face lane is empty in the preceding topology (DR-170). The decoder reads an Extrude whose native profile is the scope's one profile-role construction group in the scope's stream, whose members each have no historical face slot or one and no active face, whose resolved slots are distinct, and which has at least one unresolved member. It infers the faces from the scope's direct transition when the Extrude is one-sided and blind along the profile normal or its reverse, starts on the profile plane, and has no draft. A preceding or inserted face with several surface bindings, a surface with several plane rows, or no surface binding, and an inserted face on a surface without geometry rows, keep the root native. A preceding face on a surface without geometry rows lies on each plane that holds all its vertices, or on every plane when its loops cannot be read, and counts there as a face that cannot be read. The inserted planar faces parallel to a plane at the blind distance from it are taken as end caps that repeat the swept profile. A preceding face can be the profile when it is a resolved face, when the transition deletes or updates it, or when the Extrude makes a new body. On each such plane and side whose preceding faces can be the profile, the preceding planar faces whose interior sample points lie inside the caps are the root's faces when they can be the profile, number one per root member, include every resolved face, cover the caps' area, and have a union outline that lies on the cap boundaries and passes through every cap vertex. Each cap and selected face edge is a line on a straight carrier or a circle where the face meets a cylinder about an axis parallel to the plane normal. A loop has no readable outline when a vertex has several point bindings or positions, a coedge several rows, or an edge several vertex rows, curve bindings, axis rows or samples, or when a neighbouring loop of a circle edge has no single owner face or that face no single surface binding. The root stays native when an inserted planar face has no unit normal, when a selection that can be the profile fails these conditions, when two such selections differ, when a cap over a plane with such a face cannot be read, or when a face whose edges or sample point cannot be read reaches into a cap and either that face or the selection on its plane can be the profile. Such a face is drawn by its outline points, or by its vertices when its edges cannot be read. It reaches into a cap when its loops cannot be read, when a point is not finite, when none of its loops has three points, when one of its points lies inside the cap away from the cap boundary, when a cap vertex lies inside the face away from its loops, or when the mean of its largest loop's points or a point of a grid that divides its bounds into 24 parts on each axis lies inside both the face and the cap, away from their boundaries. This reading reports `feature.extrude-profile-cap-faces-inferred` with the faces at the blind length outside the caps, the unreadable faces clear of the caps, and the number of planes and sides whose faces no member resolves and the transition neither deletes nor updates. When no cap selects such a face set and at least one member is resolved, the transition's deleted faces are the root's faces when they are as many unique preceding planar faces as root members, include every resolved face, and lie on the plane of the resolved faces. A Cut can also delete coplanar faces outside the selection; this reading reports `feature.extrude-profile-deleted-faces-inferred` with the coplanar faces that the transition updates and keeps and the candidates that the end-cap search passes over. When both readings give a face set and the sets differ, the root stays native. Each resolved member keeps its face at its member position, and the remaining faces take the unresolved positions in increasing stable face-slot order; a profile with two or more unresolved members reports `feature.extrude-profile-face-order-assumed`.

**Need.** The node grammar and the empty-lane binding of DR-170, so that each member's recipe selects its face.

**Conflict.** None.

**Note.** On a plane whose preceding faces can be the profile, a search past a local cap of 256 faces or 256 caps on the plane, 4096 outline points, or 4096 edges on one face keeps the root native and reports `feature.extrude-profile-face-search-capped`. A root that stays native for a contradicting or unreadable face set, or for inferred faces that fill no member order or historical selection, reports `feature.extrude-profile-face-set-disputed` with the reason.

### DR-304. Fallback faces of Extrude bounded-face operands

**Question.** Which faces does an `Extrude` bounded-face operand select when its recipe nodes have no unique topology binding?

**Known.** The nodes of a bounded-face recipe name, by prefix-operand ordinal, faces around the boundary of the recipe's face. Without a unique binding, the decoder selects faces with the candidate and support rules of `f3d.md` §3.1 that follow the alternate-selector proof, and tests the selection unless the grouped Design-reference intersection holds a face or the operand has alternate-selector candidates. The test takes the prefix operands that the nodes with two side clauses name, by root, first prelude value and fan values, or every prefix operand when no node has two side clauses, as naming faces around the selected face's boundary; any other node, or one that names an ordinal outside the operands, names none. Such an operand supplies the preceding-topology faces that its references with a nonnegative Design reference name; it contradicts nothing when a negative reference of it names a face, when another reference names no face, when a named face lies outside the preceding topology, or when a named face has no rebuilt boundary. The selection fails when every selected face has a rebuilt boundary and the selection shares no boundary vertex with the faces of one operand. A failing selection is withdrawn. The legacy start and termination candidate rule then gives the operand only a face that is not withdrawn and shares a boundary vertex with every such operand. The profile-group carrier rule pairs each member with the face of the same position in increasing stable face-slot order; it takes no face set that holds a face withdrawn from one of its members, or that pairs a withdrawn member with a face that does not share a boundary vertex with every such operand of that member. Each refused face or face set reports `feature.face-operand-fallback-contradicted`. The sketch-plane start and termination rules of the same scope can still select any candidate face, the withdrawn one included.

**Need.** The node grammar and the empty-lane binding of DR-170, so that each recipe binds its own face.

**Conflict.** `f3d.md` §3.1 resolves such an operand to the sole preceding candidate, or to the sole candidate that the transition chain changes. In a cut of two split top faces, that candidate is a side face that shares no boundary vertex with the faces of the recipe's prefix operands.

**Note.** A withdrawn selection reports `feature.face-operand-fallback-contradicted` with the faces of the operand that it does not touch.

### DR-171. Sketch support identity

**Question.** Which placement references join a Sketch to its supporting plane or historical face and the feature that produces that support?

**Known.** A Sketch scope references a paired placement record. Placement construction records carry a solved matrix, direct geometry references, and a base-level payload with additional record references. The 329-byte Sketch placement frame stores its origin in centimetres. The neutral Sketch frame origin is in millimetres.

**Need.** Controlled Sketch supports that independently vary the producing feature, selected face, and offset plane. The support identity must follow changes to the producing feature's parameters.

**Conflict.** None.

**Note.** DR-12 tracks placement construction types. Sketch support identity remains in the native placement construction record.

### DR-200. Fields of the class-387 and class-329 Combine prologues

**Question.** Do the class-387 and class-329 `Combine` prologues store the fields of the operand-list prologue?

**Known.** `f3d.md` §3.1 reads the operation at offset 21 with values one through three and keep-tools at offset 25 in the class-387 form, and reads keep-tools at offset 30 and the operation at offset 31 in the class-329 form. The operand-list form stores a zero-based operation at offset 21, keep-tools at offset 29, and u32 one at offset 31.

**Need.** Controlled class-387 and class-329 frames, or a type-table rule that separates them from the operand-list form, with stored operations proved by result volumes.

**Conflict.** A class-387 frame with zero at offset 25 and bytes `01 00` at offsets 29 and 30 also satisfies the operand-list fixed fields. The two readings give different operation and keep-tools values. The rule that resolves a stored `Join` with kept tools to `Cut` applies only to the older forms and can follow from this difference.

**Note.** Class tags are local to a stream, so a class value does not identify a frame layout.

### DR-201. Byte after the hidden flag of a Combine browser node

**Question.** What does the byte after the hidden flag of a browser node named by an operand-list `Combine` state?

**Known.** The byte is zero or one. It is zero when the tool body was hidden before the `Combine` and the feature consumes the tool. It is one in the other observed frames, including a consumed tool that was visible before the `Combine` and a kept tool that was hidden before it.

**Need.** Controlled bodies that vary visibility before and after the `Combine`, with keep-tools varied independently.

**Conflict.** None.

**Note.** The body-visibility join in `f3d.md` admits only the `0x01 0x01` member form.

### DR-271. List that a two-list Combine prologue omits

**Question.** Which of the three lists between the tool list and the target list does an operand-list `Combine` prologue omit when it stores only two?

**Known.** `f3d.md` §3.1 gives the operand-list prologue: after the tool list, an empty list, a list that is empty or names the new component's browser node, the consumed-body list, and the target list. A prologue with a companion `RemoveBody` can store two empty lists between the tool list and the target list. A `Combine` with a companion consumes no tool in its own scope. The decoder reads every tool of such a `Combine` as removed. It compares the browser nodes that the tool selections name with the browser nodes that the body selections in the companion reference tables name. When the two sets differ, or a companion has no single reference table after the GUID or a selection without one browser node, it reports `feature.combine-tool-consumption-assumed`, whose note lists the tool selections that no companion selection confirms and the companion selections that name no tool. With the new-component Boolean zero, the reading of every stored field is the same for each omitted list; with the Boolean one, only the omitted browser-node list is consistent. The decoder reads a two-list prologue with a companion and two empty lists, and reports `feature.combine-new-component-native` when the Boolean is one.

**Need.** A two-list prologue with a nonempty list between the tool list and the target list, or the Fusion version that adds the third list.

**Conflict.** None.

**Note.** None.

### DR-272. Prologue of a Combine frame whose byte 20 is one

**Question.** Which prologue does a `Combine` frame store when byte 20 is one and its fixed fields match both the standard and the operand-list prologue?

**Known.** `f3d.md` §3.1 gives the standard and operand-list prologues. A standard join and an operand-list prologue with an empty property block both store `01 00 00 00 00` at offsets 20 through 24, and such an operand-list prologue without a companion also stores zero at offsets 26 through 32, as the standard prologue does. When the standard prologue reads such a frame, the decoder reads it in that prologue and reports `feature.combine-prologue-assumed`, whose note says whether the lists name the scope's operation-identity records. Otherwise the decoder reads the frame in the operand-list prologue when its lists name those records, its counted GUID and three zero bytes end at its reference table, its other fields hold, and its new-component Boolean is zero. A frame that neither prologue reads stays native: it reports `feature.combine-new-component-native` when its new-component Boolean is one, and `feature.combine-operand-list-refused` when its lists name its operands and its other fields contradict the operand-list prologue or cannot be checked.

**Need.** A frame of this kind with its operation and keep-tools state shown by result volumes.

**Conflict.** None.

**Note.** None.

### DR-202. Fields of the older Shell frames

**Question.** Do the fixed-length `Shell` frames with a Boolean at offset 21 store the fields of the wall-slot form?

**Known.** `f3d.md` §3.1 reads offset 21 as the outward convention in the class-369/261 frame, and the decoder reads the same offset that way in the other fixed-length frames. The wall-slot form stores the tangent-chain Boolean at offset 21 and states the wall direction by which slot is present.

**Need.** Controlled frames of the fixed-length forms that vary the wall direction and the tangent chain independently, with result volumes.

**Conflict.** One byte offset has two meanings across the forms. The two meanings give different wall directions for a tangent-chain selection if the older frames store the newer field.

**Note.** No authored file has a fixed-length frame.

### DR-204. Row default flag and revision members of configuration tables

**Question.** What do a row `default` value `true`, a column `isCustom` value `true`, a nonempty `cachedRevIds` object, and the row `configRevisionId` state in a configuration table?

**Known.** Every row in the populated tables stores `default` `false` and a `configRevisionId` UUID that differs from the row id. Every column stores `isCustom` `false`, and `cachedRevIds` is `{}`. These values stay the same when the active row, one parameter cell, one suppression cell, one row name, one row copy, or one column target changes.

**Need.** A saved design with a row marked as the default, a design with a customized column, and a design saved again after changes to configured rows.

**Conflict.** None.

**Note.** The decoder retains these members in the native table document. A `default` or `isCustom` value `true` and each member of `cachedRevIds` emit `configuration.member-unassigned`.

### DR-205. Entries of a modeled extended Thread prefix

**Question.** What do the two u64 values of each 16-byte entry after the entry count of an extended Thread prefix state?

**Known.** A modeled extended Thread stores count `1` and one entry with u64 `1` and u64 `0x130`. The Design stream of that thread has no indexed record `0x130`. A cosmetic extended Thread stores count `0` and no entry.

**Need.** Modeled threads on several faces, on bodies of different sizes, and in designs with different record counts.

**Conflict.** None.

**Note.** The decoder admits entry counts `0` and `1`, reads the thread class after the entries, and keeps the entry bytes only in the retained source image.

### DR-206. Sides of a two-sided to-object Revolve

**Question.** Which side of the profile plane does each target of a two-sided to-object `Revolve` end?

**Known.** The frame stores the side-one and side-two target groups and no side flag. Two scopes whose side-one and side-two target planes are exchanged store the same frame and produce the same body. In each two-sided scope, each side of the profile plane ends at the first target that its travel reaches, and the two sides reach different targets first.

**Need.** Two-sided to-object scopes whose travels on both sides reach the same target first, and scopes whose targets are faces or bodies.

**Conflict.** None.

**Note.** The decoder keeps both target groups in the native construction and projects a two-sided extent whose two terminations are unresolved.

### DR-172. Paired-locus dimension owner binding

**Question.** How does a paired-locus frame assign its dimension when its explicit owner reference names a later dimension and its paired header is followed by a parameter record?

**Known.** One public FHCS `d9` companion contains a three-locus frame with a null locus of role `6` and two point loci of roles `0` and `1`. Its return run repeats the two point references in reverse order. The explicit owner reference names the owner of `d10`, whose expression is `d9`. The paired header is followed by the `d10` parameter record and then its owner. The decoder retains the `d9` companion as a native dimension operand. The two `d10` distance constraints and the `d5` angular constraint have neutral operands.

**Need.** Controlled linear dimensions with and without a dependent dimension must establish the relation between the containing companion, the explicit owner reference, and the following parameter and owner records.

**Conflict.** None.

**Note.** The public FHCS frame selects points with persistent identities `117` and `119`.

### DR-240. Presentation frame whose label states another value

**Question.** Which dimension does a direct presentation frame govern when its label states a value that differs from the first following dimension owner's parameter?

**Known.** The opaque presentation bytes of a direct frame hold an LP-UTF16 label that states the governed parameter's expression. Under the first-following-owner rule, one parameter owner can receive two frames, and a frame can receive an owner whose parameter differs from its label. When one owner has several frames, the frame whose operand measurement equals the parameter governs it.

**Need.** Files with the direct two-operand frame form that create a dimension, delete or replace it, and save the document after each change.

**Conflict.** None.

**Note.** One public frame states `91.675 mm`, its two points are 91.675 mm apart, and its first following owner is a 5.175 mm parameter that a second frame also selects.

### DR-241. Byte 29 of a 262-byte zero-body Base Feature frame

**Question.** What does byte 29 state in a 262-byte `Base Feature` frame whose layout otherwise matches the 262-byte zero-body form?

**Known.** Layout record `base_feature_wide_zero_body` in [the layout tables](../layouts/f3d.md) defines offsets 21 through 35 as zero. One public frame stores `1` at offset 29 and zero in the rest of that run. Its current and preceding history states are equal. The decoder retains it as an unbound stored-body scope. Two controlled one-body `Base Feature` saves from the current Fusion release store `1` in the matching byte: a 331-byte class-260/258 frame whose body is built in place, and a 316-byte class-368/258 frame whose body is a copy of a body in a referenced design. In each frame the run `u8 1`, eight zero bytes, this byte, and `u8 0` precedes a u32 body count `1`; the byte is at offset 63 in the first frame and at offset 48 in the second. Neither frame length is a form in `f3d.md` §3.1.

**Need.** Saves of one design that differ only in this byte, for example across Fusion releases or design options.

**Conflict.** None.

**Note.** Two other public frames of this form store zero at offset 29. Unbound result references remain a native body selection of a neutral Base Feature and report `feature.stored-bodies-unbound`.

### DR-251. Compact null-owner parameters: owner and trailer byte 3

**Question.** Does the null owner of a compact null-owner dimension parameter mean that the parameter has no owner or that the frame does not store it, and what does byte 3 of the five-byte compact expression trailer state when it is `1`?

**Known.** The specification states the compact null-owner parameter form and the class-`287` form, which both admit the trailers `00 00 00 00 00` and `00 00 00 01 00`. Five Linear Dimension parameters in the compact null-owner form store `00 00 00 01 00`; their expressions chain the parameters (`6.35 mm`, then each names the previous one). The compact owned trailer stores the driven flag at byte 1, which aligns with byte 6 of the wide expression trailer; under that alignment, byte 3 is the position of the wide favorite flag. These frames store the null owner `u8 0` at offset 30 while their source kind names a Linear Dimension. The decoder reads them without an owner binding and reports `parameter.owner-unstored` for each stream with such a parameter. It keeps the parameter flags at their defaults and reports `parameter.trailer-flag-uninterpreted` for each stream with a decoded parameter whose byte 3 is `1`.

**Need.** Fusion's own reading of the five parameters (favorite, driving state, owner) from a local import of a public design that stores this form, or a second public design with the compact null-owner form or a compact trailer that sets byte 3.

**Conflict.** None.

### DR-175. Ownerless fixed Fillet lanes across several operand groups

**Question.** How do ownerless fixed scalar lanes assign radius laws to several role-`0x0000000800000000` Fillet groups?

**Known.** The scalar records name the owning scope and have consecutive owner-local ordinals. Their scope reference table interleaves the scalar records with several counted operand groups. A scalar run can contain two values with two groups, or three values with three groups. The fixed single-group grammar reads these runs as a tangency weight and constant radius, or a tangency weight and two endpoint radii.

**Need.** A record discriminator or explicit assignment that separates per-group radii from a shared tangency weight and a multi-value radius law.

### DR-166. Pathless occurrence-placement control word

**Question.** What does the u32 after the empty path and separator in a pathless occurrence placement select?

**Known.** One public assembly member stores value `1` in the 25-byte prefix. Its two metadata UUIDs equal the UUIDs of the corresponding repeated-target placement in a second public assembly. Its typed target, role, rigid matrix, and local owner close the placement frame. The decoder admits control value `1` and retains the complete frame.

**Need.** Pathless placements with another control value and an independently established placement relation.

**Conflict.** None.

**Note.** The empty-path control remains native. The target role binds the external placement independently of this control's meaning.

### DR-174. Ellipse-extrusion support chart and solved cache

**Question.** How does the neutral model retain an ellipse-extrusion support and its solved surface cache when each has a different U parameterization?

**Known.** `asm.md` §6.4 "A pcurve in a procedural support slot" and `asm.md` §6.6 "The compact layout above" define the native ellipse-angle U coordinate and the rational surface-cache parameterization. A support pcurve uses the construction chart. The decoder retains that native pcurve and the owning curve's solved 3D carrier. It leaves the support surface unbound and reports `geometry.procedural-support-chart-unresolved`.

**Need.** A neutral representation that retains both parameterizations and the cache fit contract, or an exact map from the construction chart to the cache chart.

**Conflict.** `cad-ir.md` applies a procedural carrier's solved cache at the construction parameters. Rational conic arcs agree with the analytic ellipse in position and use different parameters between arc endpoints.

**Note.** ASM GC-40 records the corresponding neutral-model question for a revolved conic.

### DR-176. Saved current-body state outside consumer selections

**Question.** Which saved state determines current body membership when the Body container, Design browser visibility, and terminal ASM state retain a body that the producer omits from its current view and STEP export?

**Known.** `f3d.md` §3.1 defines the Body-container membership and the consuming-scope join through whole-body selections and browser nodes. The separate browser segment carries body-entry state bytes. A state byte can distinguish body groups whose Design browser hidden flags agree. The byte's relation to body activity, display visibility, and timeline rollback requires a controlled comparison.

**Need.** Save a small design with several Base Feature bodies and a later new body. Save it before and after one multi-select Remove of several Base Feature bodies, then after a second Remove with another selected set. Record the visible and exported body counts at each save. Make separate save pairs that change one body's visibility, suppress and restore a Remove, and move the timeline marker before and after each Remove and to the timeline end. Keep geometry fixed within each pair. At each save, track each body's Design entity suffix, BREP basename, ASM body key, Body-container membership, consumer selection, browser state byte, and bound feature outputs. Compare complete Design and browser entries, selection reference runs, saved scene visibility, and ASM state membership. Establish which byte changes with removal independently of visibility and rollback, and verify its identity join on an independent design before using it to omit a body.

**Conflict.** Local corpus files retain ten additional bodies in the Body container and terminal ASM state while the producer's current view and STEP export contain five bodies. The complete registered whole-body selection family has 36 records; all 36 decode. Its two Remove selections name two distinct browser nodes already covered by the consuming-scope rule.

**Note.** Preserve bodies while the browser-entry state and timeline endpoint remain unresolved. Creator family and container order supply comparison groups for the controlled saves.

### DR-242. Thread face continuation through later topology rewrites

**Question.** Which stored relation carries a Thread selection from its result face to the selected region after a later operation rewrites that face?

**Known.** The ASM transition chain preserves stable entity slots across record updates. A later transition can retain the threaded cylinder's slot, axis, and radius while it updates that face, deletes another face, and inserts a replacement face. The Thread selection names its preceding topology.

**Need.** Public controlled Thread designs followed by face splits, face deletion, and face motion, with the selected threaded regions identified before and after each operation.

**Conflict.** None.

**Note.** DR-143 tracks the Thread modeled state. The active-face proof requires topology and geometry preservation after the Thread result.

### DR-244. Treatment edge proof without a transition

**Question.** Which stored relation selects a constant-radius treatment edge
when the operation transition carries no candidate for the group members?

**Known.** `f3d.md` §3.1 "The transition of a constant-radius `Fillet` feature states its edges" gives the transition proof. `f3d.md` §3.1 "A recipe-backed constant-radius group resolves by a chain proof" gives the radius-qualified chain and the single-member subset rule. `f3d.md` §3.1 "An edge operand whose program does not decode as the standard structured tail has six fallback candidate proofs" gives the operand-level proofs over changed reference-edge sets.

**Need.** Controlled constant-radius treatments whose transitions carry no candidates, varying member count and deleted-edge neighborhoods, with the selected edges identified before and after each operation.

**Conflict.** None.

**Note.** Operand-level selector, reference, and boundary-count proofs operate on changed reference-edge sets; the missing proof is a group-level assignment from those sets to deleted edges without transition evidence. A multi-member group without that assignment keeps its native selection.

DR-481 tracks a Chamfer group with two bounded-face members and a nonempty transition candidate set.

### DR-247. Top-level manifest lineage suffix semantics

**Question.** What meaning do the code units in the fixed terminal lineage suffix carry?

**Known.** `f3d.md` §1.3 defines the exact suffix bytes and their position after the terminal lineage URN. The suffix decodes as five UTF-16LE code units `HAAHA`.

**Need.** Public documents with controlled metadata changes and the same terminal form.

**Conflict.** The byte sequence permits several scalar partitions.

**Note.** The reader preserves the complete manifest in the native archive payload.

### DR-249. Saved assembly state and joint activation

**Question.** Which saved assembly state selects the occurrence poses for an Assemble joint relation, and what activation state do the two scope-tail booleans encode?

**Known.** The version-three connector equation uses scope frames, four referenced angle and XYZ lanes, and facing. Ordered occurrence keys select the innermost child within one insertion, including inserted documents. The corrected output has 383 new V1.8 joint instances: 5 have a consumer-pose residual above 0.1 mm, and 6 exceed that threshold under current carriers. RailCore v1 has 3 new instances and 0 consumer residuals above 0.1 mm. The 46 incomplete V1.8 bindings included 45 open relations and one closing relation. RailCore v1 records 3068 and 3232 and V1.8 record 4176 store `01 01` in the two tail booleans and retain native alignment. Record 4176 closes. The earlier version-three control has terminal bytes `00 00`; the six version-four controls have an extra preceding `01` marker followed by `00 00`. Projection assumes every projected Assemble joint is active, supplies `suppressed = false` independently of these bytes, and charges `assembly.joint-activation-assumed`.

**Need.** A byte-level saved-state selector with links from joints to occurrence-pose carriers. Public states that change each tail boolean independently and establish the joint's activation state across independent designs.

**Conflict.** The full-path audit retains consumer-pose residuals on V1.8 records 12428, 12811, 11527, 2578, and 2333. Their distances are 17.8801954, 42.9906967, 40.0011250, 373.6835274, and 373.6835274 mm. V1.8 record 146871 has a 6.50691939 mm current-carrier residual and a consumer residual below 0.0001 mm. RailCore v1 records 3068 and 3232 have residuals of 40 mm and 18.35 mm. The earlier admitted control at V1.8 record 985 has a current-carrier residual of 0.0405289814 mm.

**Note.** Scope-level closure counts describe selected carrier pairs. Emitted-joint closure uses the actual neutral occurrence containers and complete insertion paths. Nonzero tail states retain native alignment with a partial report. Admission uses byte grammar and binding evidence independently of pose closure. Robot record 4614 has a 1472.38643012 mm current-carrier residual because its inserted occurrence stores the identity; DR-116 covers it. DR-113 and DR-116 cover their separate placement questions.

### DR-250. Motion of a joint record shared by several Assemble scopes

**Question.** When several version-3 `Assemble` scopes reference one joint record, which scope does its motion record bind, and do the other scopes take the same freedoms, limits, and rest state?

**Known.** An adjacent joint record has an index one less than its scope's, and its motion binds that scope. Seventeen V1.8 scopes reference an earlier joint record. V1.8 stores no freedom entry in any Design stream, so no shared joint shows a motion.

**Need.** A public design whose shared joint record has a motion record with freedom entries, with the moving scope identified.

**Conflict.** None.

**Note.** These scopes keep their frames, facing, angle, and offsets natively and project no neutral joint until the binding is defined.

### DR-246. Additional version-five Assemble owners and extended spans

**Question.** Which version-five fields encode additional parameter owners and the form with 22 extra bytes?

**Known.** The four-owner version-five grammar has explicit angle, Z, X, and Y references, two zero bytes after the angle reference, and six zero bytes after the owner run. Version-four scopes store connector frames consecutively, then the alignment references and a counted limit-owner run. Their four-owner form follows the version-four grammar in §3.1 of `f3d.md`.

**Need.** Public version-five controlled files with five and six owners, and a controlled pair whose scopes differ by 22 bytes. Vary one joint limit or owner-producing setting at a time. Identify each added reference target, its scalar lane, the run count, and complete paired-header closure.

**Conflict.** None.

**Note.** Extended version-five scopes retain the opaque scope envelope and its uninterpreted-payload loss.

### DR-248. Closing reference and owner of unadmitted legacy placements

**Question.** How does the closing reference of a legacy placement carrier identify its owner, including a 531-byte carrier whose closing target differs from `4`? Which saved state supplies the owner frame, and how do the instance key and metadata UUIDs bind its six-name envelope?

**Known.** The 403-byte identity and 531-byte matrix envelopes in §1.4 close with target value `4`. One local archive has eight XREF rows with undecoded placements; five target members decode, and five role-matched 531-byte carriers have nonidentity matrices and other non-null closing targets. Two public OSII Mini archives each have 43 refused version-2 Component placement carriers: two 404-byte identity spans and 41 532-byte matrix spans. The plain prefix has an extra byte, with values `0` and `1`, before its metadata count. Its closing references select five local owners. Each of the 82 matrices has a byte-identical copy in another form in its Design stream. Each root also has seven unadmitted 234-byte local identity carriers. Two local component paths retain translated frames while the corresponding local carriers store identity; one translation is `-3.6` cm on Z. One public Meanwell assembly has eight refused version-2 six-name matrix carriers: one 740-byte span and seven 744-byte spans. Each matrix also occurs in a CommonData path that names its placement carrier. Each closing owner is the document root. The six stored names are `Appearance`, `Geometry`, `PhysicalMaterial`, `Properties`, `StructureChange`, and `Transform`. These three public roots have zero saved display scenes and zero joint scopes. Their baseline losses are respectively 43, 43, and 8 `xref.placement-undecoded`, and 14, 14, and 5 `xref.placement-identity-assumed`. Multiple carriers share a reference role. In one measured assembly, nine decoded placements share a unique admitted identity owner carrier to the document root; its carrier chain supplies its external children's world frame. The three-property control-1 identity grammar is in §1.4. An undecoded XREF placement in an XREF-only table retains identity and emits `xref.placement-identity-assumed`. In a table with DERIVED rows it stays native and emits `xref.occurrence-unplaced`. A decoded placement with an unresolved owner retains its document-root placement and emits `xref.owner-unresolved`.

**Need.** For a 531-byte carrier with a closing target other than `4`, public saves that vary the closing target and the owner transform separately, with a retained display scene or closing joint for each placement. These saves must show how the closing reference selects the owner and how the owner places the stored matrix in the document frame. For the plain envelope, save a public assembly with two distinct local owners and one asymmetric external child in each. Put owner A at identity and owner B at translation `(11, 13, 17)` cm with a 180-degree rotation about root Z. Put each child at translation `(1, 0, 0)` cm in its owner frame. Save the baseline, translate owner A by `(2, 3, 5)` cm, rotate it by 90 degrees about root Z through its own origin, then move its child by `(7, 0, 0)` cm in the owner frame, with a separate save after each change. Repeat the child below a second instance of its owner. Keep the child fixed while changing the active captured position, and save both positions. Retain a display scene or dimensioned mating geometry for each save. These pairs must distinguish the stored child frame, the owner's current frame, and the selected captured state, and must close the local owner-carrier grammar. For the six-name envelope, make a second public assembly with an asymmetric inserted part, two instances of that part, and an instance below a moved local owner. Save each insertion separately, add controlled copies until the stored instance key gains one decimal digit, change one tracked channel at a time, and save before and after a captured-position change. Add one fixed joint to each instance in separate saves with the poses held fixed, and compare its operand path to the instance key and both metadata UUIDs. Keep all three repeated target references, the counted name run, the instance key, both metadata UUIDs, and the closing owner in the comparison. Require a complete indexed span, registered type and version, occurrence binding, and an independently established owner-relative pose for each admitted form.

**Conflict.** None.

**Note.** The public Voron Trident `z_mounts_front.f3d` stores 55 registered version-2 Component placement entities, 43 bound component naming spaces, and 34 component paths. Its local occurrence set is empty. Definition context bindings identify component owners independently of these placement frames. Equal matrix bytes corroborate a stored frame. Selection of an owner's current frame and of a captured state remains part of occurrence placement. The identity fallback records the unresolved placement proof.

### DR-252. Saved scene framing and state selection

**Question.** How do enclosing node and attribute frames delimit the saved world stream, which saved state do displayed overrides represent when their poses differ from Design carriers, which scene paths descend beyond the neutral occurrence tree, and what does the tagged path suffix name?

**Known.** Transform rows close through their complete local grammar in `f3d.md` §1.1.4. Robot has 39 root-scene transform rows and 22 member-scene rows. Its world also stores `OverrideInActivatedVisibilityAttribute`; one such path has five elements. Trident stores 464 `FOverrideProteinEffectAttribute` names in the same Component serializer region as 1239 transform rows. Nested world objects contain `WorldSerializerEndMark` strings. Local corpus files store override paths with a tagged pair suffix of up to 14 elements. The low indices of these pairs match no carrier of a neutral occurrence under the untagged prefix.

**Need.** Public controlled saves that vary one node attribute, the active captured position, or one occurrence depth at a time. Pair each stored scene with its Design carriers and complete serializer boundaries.

**Conflict.** Displayed matrices and stored Design world poses can differ.

**Note.** Enclosing node framing is not decoded. The local transform reader leaves visibility and material override attributes unread. Binding uses the root design scene; member scenes retain their separate saved state.

### DR-253. Typed occurrence path identity-word variants

**Question.** What do the eight-byte word between the two identity-GUID pairs and the eight-byte tail after the final u32 `2` mean in a positive-count typed occurrence path, and how do they affect binding?

**Known.** The admitted identity suffix in `f3d.md` §3.1 "A class-329 path begins with its indexed header" stores eight-byte value `2` between the two GUID pairs and an eight-byte zero tail. Twenty-six live version-4 paths of type `96CBEA21-17E0-4BCB-BA77-04EA87B47934`, in four documents, store `0x0000000100000002` and `1` at those positions. Each path has one or two occurrence keys and four identity GUIDs. Each 425-byte path is bounded by its indexed wrapper. The paths belong to thirteen 806-byte Assemble scopes. The primary record index fixes their live offsets. Their scopes retain explicit connector matrices and the `assembly.joint-unprojected` loss. On all thirteen scopes the occurrence keys resolve, and the connector frames at the current occurrence poses, with the identity for an occurrence that stores no pose, satisfy the joint relation in "Assembly operands" in rotation and translation. These scopes do not separate the two word pairs, because no such scope stores `(2, 0)`.

**Need.** Make controlled saves of one joint between two fixed component occurrences. Give each occurrence an identity-transform child with a coincident connector. In separate saves, change the first operand from its root connector to its child connector, then change the second operand in the same way. Hold both connector matrices fixed. In further independent saves, update only the first component revision, only the second component revision, and only the joint suppression state. Create two component configuration rows with equal geometry and equal connector matrices; switch each operand between those rows independently. Compare both identity words and all five GUID fields with the baseline. Obtain both word pairs `(2, 0)` and `(0x0000000100000002, 1)`. Establish which controlled changes affect each word and whether the words can vary independently before projection.

**Conflict.** The nonzero words change the admitted identity suffix. Treating them as padding would discard stored values.

**Note.** The current path decoder retains these scopes natively. Their thirteen paired-frame findings remain errors until the identity state has a complete grammar and binding rule.

### DR-255. Decal target selected by a face-recipe entity selection

**Question.** What does a Decal target group carry when a member is a persistent entity selection whose identity record holds a face recipe instead of a whole-body recipe envelope, and which faces does it select?

**Known.** `f3d.md` §3.1 "A `Decal` scope is a presentation timeline object" gives the scope prefix layouts and a target group with role `0x0000000400000000` whose whole-body recipe envelope members identify faces. A target group can have a member `N` of another form. The records `N` through `N + 3` follow the entity-selection frame: a prefix with an asset GUID and a context GUID, the paired header `N`, and the nested records `N + 1` and `N + 2`. The identity record `N + 3` is 90 bytes long and contains a `face_recipe_data` program. Record `N + 4` stores the same asset and context GUIDs. The entity-selection frame grammar admits 29-byte and 45-byte identity records and the 49-byte class-338 sketch-curve identity with a class-361 identity record, so this member decodes as neither an entity-selection operand nor a body-recipe operand.

**Need.** Make controlled saves of one Decal on one planar face and on one cylindrical face. Compare the target member records with the whole-body recipe form. Establish the grammar of the 90-byte identity record and of record `N + 4`, and how the face recipe resolves to faces.

**Note.** The decoder projects the Decal with its image and a native face selection. It records `appearance.decal-target-unresolved`.

### DR-484. Decal mapping modes other than `0x60`

**Question.** Which image mapping does each Decal mapping-mode byte other than `0x60` select?

**Known.** `f3d.md` §3.1 "A `Decal` scope is a presentation timeline object" places the u8 mapping mode between the image-asset reference and the target-group reference and defines mode `0x60`. Decal scopes also store the modes `0x00`, `0x01`, `0x7D`, and `0xE2`. A larger indexed record with the image-asset record index precedes the image-asset record. It stores f32 coordinate pairs and u32 index runs.

**Need.** Make controlled saves of one Decal that change one placement control at a time: scale, rotation, translation, each flip, and face chaining. Compare the mapping-mode byte and the larger same-index record across the saves.

**Note.** The decoder projects the fit-to-faces mapping for every mode. It records `appearance.decal-mapping-assumed` for a mode other than `0x60` and keeps the byte in the native `mapping_mode` field.

### DR-256. Ordinal prefix of a leading-flag local component occurrence

**Question.** What does the u16 ordinal prefix at offset 44 of a leading-flag local component occurrence state?

**Known.** The prefix is `1` for the first new component of a Design and for a pasted second instance of that component, and `0` for a later new component. Its value is independent of the occurrence's grounded, light-bulb, visibility, active, and isolated states. The attributed frame stores two bytes after its attribute run, before its ordinal.

**Need.** A Fusion control that changes the prefix of one occurrence with all other carrier fields equal.

**Conflict.** None.

**Note.** The decoder keeps the prefix in the native occurrence frame.

### DR-268. Loft setting of the standard Loft scope marked reference

**Question.** Which Loft setting selects a non-null marked reference at standard `Loft` scope offset 33?

**Known.** `f3d.md` §3.1 "The compact fixed-prefix `Loft` scope has ten zero bytes" gives the reference syntax, and §3.1 "A `Loft` has at least two sections" makes the group that the target names a rail. In each scope with a non-null reference, the scope has one role-`0x0000000500000000` rail group and the target equals the record index of that group. Scopes with rail groups or a centerline group also store a null reference. The decoder keeps the target in the Loft construction, records `feature.loft-reference-unresolved`, and projects the Loft with closed false, solid true, ruled false, and linearize false.

**Need.** Saves of one railed `Loft` that change one rail or guide setting at a time, each recording byte 33 and the target, and a save whose target is not a record of the scope's reference table.

**Conflict.** None.

**Note.** None.

### DR-294. Flag bytes of the three-flag Loft prefixes

**Question.** Which Loft settings do the three flag bytes at offsets 21 through 23 of the three-flag compact `Loft` prefix and at offsets 25 through 27 of the three-flag standard `Loft` prefix select?

**Known.** `f3d.md` §3.1 "The compact fixed-prefix `Loft` scope has ten zero bytes" gives the flag positions. The three-flag compact form stores `01 01` and a byte `0` or `1`. The decoder records `feature.loft-settings-assumed` with the three flag bytes for each scope in either form and projects the Loft with closed false, solid true, ruled false, and linearize false.

**Need.** Saves of one three-flag `Loft` that change closed, solid, ruled, and linearize one at a time, each recording the three flag bytes.

**Conflict.** None.

**Note.** None.


### DR-331. Fields of the Loft scope body

**Question.** Which Loft settings do the head words, the three references, the selector, and the six bytes between the rail list and the third reference of the `Loft` scope body select, and what does the section record `N` of a section-list entry store?

**Known.** `f3d.md` §3.1 "The `Loft` scope body starts after the result-operation u32" gives the field order. The first head word is `-1` or `1`. A nonzero selector precedes a non-null second reference, which names a parameter owned by the scope. The decoder reads the rail list and the section list, takes construction-operand group `N+1` as the section group of section-list entry `N`, keeps the other fields in the scope bytes, and projects the Loft with closed false, solid true, ruled false, and linearize false.

**Need.** Saves of one `Loft` that change one end condition, one guide setting, and closed, solid, ruled, and linearize one at a time, each recording the scope body.

**Conflict.** None.

**Note.** None.

### DR-332. Centerline of a Loft rail list

**Question.** Does a rail list whose only entry is a role-`0x0000000700000000` group identify a centerline or one rail?

**Known.** `f3d.md` §3.1 "A `Loft` has at least two sections" gives the rail list. A rail list with two or more entries can hold role-`0x0000000700000000` groups, and those groups are rails. When the rail list has one role-`0x0000000700000000` entry and the offset-33 reference is null, the decoder projects that entry as `LoftGuidance::Centerline` and records `feature.loft-centerline-assumed`.

**Need.** One save of a centerline `Loft` and one save of a one-rail `Loft` whose rail is a model edge, each recording the rail list and the group roles.

**Conflict.** The decoder assigns the centerline from the role of a sole rail-list entry, and role-`0x0000000700000000` entries of longer rail lists are rails.

**Note.** None.

### DR-313. Prologue flag byte of the Thicken scope frame

**Question.** Which Thicken setting does the prologue flag byte of the six-byte marker-one prologue of a `Thicken` scope select?

**Known.** `f3d.md` §3.1 "A `Thicken` scope primary frame stores nine zero bytes" gives the byte position and the two settled flag bytes. Saves that change the thickness sign, the symmetric direction, and tangent-chain selection store prologue flag byte `0`. The decoder records `feature.thicken-flags-assumed` when the byte is `1` and projects the Thicken from its symmetric flag and its thickness; the tangent-chain flag stays on the native record.

**Need.** Saves of one `Thicken` that change one further setting at a time and store prologue flag byte `1`.

**Conflict.** None.

**Note.** None.

### DR-314. Optional u32 zero of the Thicken scope frame

**Question.** What does the u32 zero that can follow the marked scalar reference of a `Thicken` scope encode, and which scopes store it?

**Known.** `f3d.md` §3.1 "A `Thicken` scope primary frame stores nine zero bytes" gives the field and the u32 `1` before the marked auxiliary reference that places it. The decoder reads scopes with and without the field and keeps no value from it.

**Need.** `Thicken` scopes that store a nonzero value in the field, or saves of one `Thicken` from application versions whose frames differ in the field.

**Conflict.** None.

**Note.** None.

### DR-269. Marker-one Revolve frames of other lengths

**Question.** Which field layout does a `Revolve` scope use when it has the marker-one prologue and a token-independent frame length other than 345, 355, 367, 351, or 373?

**Known.** `f3d.md` §3.1 gives the marker-one form a 345-, 355-, or 367-byte token-independent frame, and the referenced-angle form, which shares the marker-one prologue through offset 33, a 351- or 373-byte frame. The decoder reads a frame of another length with the marker-one field offsets and records `feature.revolve-layout-assumed`.

**Need.** A Design stream with a marker-one `Revolve` scope of another token-independent length and the feature's parameters as the application shows them.

**Note.** None.

### DR-270. Curve order of a self-crossing stored profile loop

**Question.** Does the listed curve order of a self-crossing stored profile loop identify which atomic sketch regions the loop selects?

**Known.** No source file has been checked against this relation. The decoder takes as its hypothesis that a stored loop that matches no single arrangement face lists a curve identity for each boundary use of every selected region. When exactly one set of atomic faces uses only the listed curves, each as many times as the loop lists it, the decoder selects that set and does not read the order. A face that encloses an island is assumed not to be selected and is no candidate; at least two candidates are required. Two sets with equal counts and a curve that is not a line or circular arc leave the loop unresolved. The count rule does not distinguish selections whose faces share all curves.

**Need.** Make controlled saves of a square sketch with both diagonals. In separate saves, one through-cut Extrude selects all four triangles, one selects two opposite triangles, and one selects two adjacent triangles. Compare the stored loop entry order and counts of each save with the face boundaries.

**Conflict.** None.

**Note.** The decoder reports `feature.profile-crossing-loop-assumed` for each selection that takes regions from this rule. When an Extrude selection resolves to whole stored profile loops, alone or as regions of an outer loop with hole loops, the decoder also compares the faces its transition changed on planes parallel to the Sketch. It selects each atomic region bounded only by the loops' curves whose interior point a face on such a plane held before the transition and no face holds after it, and reports `feature.profile-cut-regions-assumed`. When no such selection exists and the Extrude is a Cut, it also takes the removed atomic regions of every non-construction Sketch curve, provided at least one taken region uses a curve outside the stored loops. A region that a plane gains refuses the reading. A region that one plane holds on both sides while another removes it refuses the reading. A region that planes hold on both sides and none removes stays unselected, and so does a region that no face on such a plane holds before or after the transition. This selection also reports `feature.profile-cut-regions-assumed`.

### DR-372. Flagged mesh scope prefix

**Question.** What does the u32 in the flagged mesh feature scope prefix mean?

**Known.** The flagged scope stores a u32 before its body count. The decoder retains the u32.

**Need.** Controlled saves that vary the flagged prefix and establish the u32 meaning.

**Conflict.** None.

**Note.** The decoder reports `mesh.scope-prefix-uninterpreted`.

### DR-478. Empty mesh-import source lists

**Question.** Does an empty body-reference run identify an import whose source was removed, or an import that introduced no body?

**Known.** A Base Mesh Feature can store body count zero while its operation scope remains in the timeline. Its scope identity identifies the retained import source.

**Need.** Controlled saves of a newly created empty import and an import whose last body was removed, with their operation scopes compared.

**Conflict.** None.

**Note.** The decoder keeps a native mesh source selection and reports `feature.mesh-import-empty-source-assumed`. Unjoined nonempty body runs report `feature.mesh-import-tessellations-unresolved`. Unreadable source frames report `feature.mesh-import-source-unparsed`. DR-292 tracks the operation GUID bindings.

### DR-373. Mesh body and collection-owner payload fields

**Question.** What do the compact matrix flags, the further mesh-body reference slot, and the remaining mesh-body, mesh-wrapper, and collection-owner payload fields mean?

**Known.** A version-6 body stores placement matrices, graph references, a display name, and additional bytes. A matrix presence byte `01` has no matrix payload. Fifteen bytes separate the second presence byte and its payload from the first presentation envelope. The decoder uses an identity matrix for this form. A body can store a zero byte in its feature-scope slot and a reference in its further reference slot. The decoder reads a zero byte in the further slot and rejects other bodies. The collection owner stores a named collection reference and other fields. A mesh wrapper can continue after its fixed prefix. The decoder retains these bytes.

**Need.** Controlled saves that vary one body or collection-owner property at a time and identify the remaining fields.

**Conflict.** None.

**Note.** The decoder reports `mesh.body-bytes-uninterpreted`, `mesh.body-identity-assumed`, `mesh.wrapper-bytes-uninterpreted`, and `mesh.owner-bytes-uninterpreted`.

### DR-305. Inferred-axis bytes of a WorkPlane placement

**Question.** What do the `isInferredXAxis` and `isInferredZAxis` bytes of a placement construction state, and does a nonzero value change the plane that the stored matrix of a `WorkPlane` placement gives?

**Known.** `f3d.md` §3.1 "A `Sketch` scope joined" gives the member positions and the solved plane of a `WorkPlane` placement whose two bytes are zero. Each byte holds a value from 0 through 255, and the two bytes vary independently. The class-pair `WorkPlane` placement frames of §3.1 hold the two bytes at offsets 45 and 46, or at offsets 62 and 63 in the direct forms. The opaque little-endian u16 of a frame stores both bytes, the class-290/262 frame stores 1 and 1, and the class-364/264 321-byte frames store 0 and 1, or `0xcc` and `0xcd`. The decoder takes the stored matrix or identity marker of a registered `WorkPlane` placement, and the matrix of a class-pair frame, as the solved frame for every value of the two bytes. It reports `work-plane.inferred-axis-assumed` when either byte is nonzero. A named-cylinder frame requires both bytes to be zero.

**Need.** Saves of one `WorkPlane` that change one construction input at a time, each recording both bytes and the plane position that the application reports.

**Conflict.** None.

**Note.** None.

### DR-286. Sketch folder flag byte, tail zero run, and closing reference

**Question.** What do the flag byte at sketch folder record offset 26 and the length of the zero run between the parent collection reference and the closing reference select, and which record does the closing reference name?

**Known.** The flag byte is `0` or `1`. Folders with either value carry sketch runs and a parent collection reference of the same form. The zero run is four bytes in a folder with an empty name and eight bytes in named folders. The closing reference is a non-null marked reference that ends the folder frame. The decoder reads the run as zero bytes of any length that ends at the closing reference, reads the closing reference target, and reports the target in `sketch.folder-closing-reference`.

**Need.** Two saves of one folder that differ only in a folder property, folders that differ only in the zero-run length, the record type of the closing reference target, and saves of one design that change the component or the folder set of a sketch folder.

**Conflict.** None.

**Note.** None.

### DR-287. Sketches omitted from their collection run

**Question.** Which stored value selects the sketches whose Geometry member names an ordinal collection that does not list them?

**Known.** Such sketches carry unique ordinals in the collection namespace, a Sketch feature scope, and a Sketch browser node. Their Sketch features follow every Sketch feature of a listed sketch in the timeline. The 960-byte timeline record does not distinguish them.

**Need.** The record that stores the timeline end position.

**Conflict.** None.

**Note.** None.

### DR-290. PathPattern settings scalars at offsets 94 and 124

**Question.** What do the f64 values at offsets 94 and 124 of a path-pattern settings record encode?

**Known.** `f3d.md` §3.1 gives the path-pattern settings record. The f64 at offset 94 is 3.0 on four line paths whose adjacent spacing is 3 cm and whose start point is at path parameter 0. It is −1.92699 on a circular-arc path of radius 5 cm with a 2 cm spacing and its start point at path parameter π/4. The f64 at offset 124 is 0.5 on that arc path and zero on the line paths. The decoder reads neither value.

**Need.** Saves that vary the spacing and the start point on one path, with the two values compared against the instance positions.

**Conflict.** None.

### DR-344. Symmetric rectangular-pattern directions

**Question.** How does a symmetric direction measure a seed-to-final-instance extent, and which byte stores the symmetric state of direction two?

**Known.** `f3d.md` §3.1 "A rectangular-pattern scope can store an indexed pattern payload" gives the symmetric byte of direction one at payload offset 109 and the zero byte at offset 110. A three-instance symmetric direction with adjacent spacing places one instance on each side of the seed at that spacing. The decoder projects that direction as a one-sided linear pattern and reports `feature.rectangular-pattern-placement-not-transferred`. A symmetric direction with an extent distance stays unresolved.

**Need.** Saves of a symmetric direction with an extent distance, of a symmetric direction with an even count, and of a symmetric direction two, each compared against the instance positions.

**Conflict.** None.

**Note.** R-Pattern 3 in `Parametric_extrusion_generator.f3d` stores zero at payload byte `133223`, the direction-one symmetric field. Its two counts are two. In this case adjacent distance and total extent give the same 15 mm spacing.

### DR-298. SurfaceTrim cell-table entries outside the removal reading

**Question.** What do the entry values and the trailing value of a `SurfaceTrim` BRep-cell table encode when the entry count equals the trailing value or two entries store the same value?

**Known.** `f3d.md` §3.1 "A `SurfaceTrim` scope has two nonempty counted construction-operand groups" gives the table layout and the removal reading for a table whose entry count is less than the trailing value and whose entry values are distinct. In such a table the entry values are `1` through the entry count. The entries of one table reference distinct indexed records. A table whose entry count equals the trailing value can repeat an entry value, for example the values `1, 1` with trailing value `2`, or `1, 2, 3, 3` with trailing value `4`.

**Need.** Controlled saves of one trim with a known removed region, where the trimming tool crosses one target face and then two target faces, and the entry values of each save.

**Conflict.** Under the removal reading, a table whose entry count equals the trailing value removes every cell of the partition, but the trim has a result body.

**Note.** The decoder keeps the entries of such a table in the native carrier, leaves the neutral `keep` region unresolved, and records `feature.trim-cell-table-native`.

### DR-299. SurfaceTrim scope lanes before the ordered reference table

**Question.** What do the lanes of a `SurfaceTrim` scope between the primary header and the ordered reference table encode?

**Known.** Nine zero bytes follow the primary header. Byte 20 is `0` or `1`. When it is `1`, a zero u32 follows it and a u32 count is at offset 25; otherwise the u32 count is at offset 21. The count equals the member count of the target group, and that many marked record references follow it. A marked reference to the tool group follows the run. A lane of 8, 12, or 23 bytes follows; the 23-byte lane holds a zero u32, u32 `1`, a marked record reference, and a zero u32. Then u32 `1`, a marked reference to the target group, and an LP-UTF16 null GUID follow. Three bytes separate the GUID from the reference count.

**Need.** Controlled saves that change one trim option at a time, with the length of the lane after the tool-group reference and the run records of each save.

**Conflict.** None.

**Note.** The decoder projects the scope from its ordered reference table and keeps these lanes in the native scope bytes.

### DR-315. Offset-21 Boolean of the compact, marked, and counted Shell prefixes

**Question.** Which `Shell` option does the Boolean at primary-header offset 21 of the compact, marked, and counted prefixes store?

**Known.** `f3d.md` §3.1 "One of four prefixes precedes the slots" defines the four prefixes. The byte is `0` or `1`. The present wall slot sets the wall direction: inside-slot scopes with either value of the byte add their walls inside the kept faces. The cornered prefix stores the tangent-chain Boolean at the same offset. The decoder stores the byte and its offset in the `prefix_flag` field of the native `Shell` operation and reports `feature.shell-prefix-flag-assumed` with the value and offset for each such scope.

**Need.** Controlled saves of one `Shell` in each prefix that change only the tangent-chain option, and saves that change only the corner type, with the byte compared across each pair.

**Conflict.** None.

**Note.** None.

### DR-316. Auxiliary references of the counted Shell prefix

**Question.** What do the counted auxiliary references after the wall slots of the counted `Shell` prefix name, and do they change the walls?

**Known.** `f3d.md` §3.1 "In the counted prefix" defines the count and the eleven-byte marked references, which are outside the ordered reference table. Each referenced record stores a 36-code-unit LP-UTF16 GUID, a marked reference, and a marked reference to the owning `Shell` scope. The decoder keeps their record indices in `auxiliary_references`, projects the `Shell` from its wall slot and face group, and reports `feature.shell-auxiliary-references-native`.

**Need.** Controlled saves of one `Shell` with zero, one, and two auxiliary references, with the result bodies, that show the user action which adds each reference.

**Conflict.** None.

**Note.** None.

### DR-330. Shell table references outside the face-group members

**Question.** What does an ordered reference of a compact, marked, or counted `Shell` scope name when it is neither the face group, the wall scalar, nor a member of the face group?

**Known.** `f3d.md` §3.1 "A u32 one, a marked reference to the face group" states that in the compact, marked, and counted prefixes the remaining ordered references are the members of the face group. The decoder projects such a `Shell` from its wall slot and face group and reports `feature.shell-table-members-assumed` with the remaining references and the group members.

**Need.** A `Shell` scope whose remaining ordered references differ from its face-group members, with the result body that shows whether those references change the walls.

**Conflict.** None.

**Note.** None.

### DR-311. Extension method of a SurfaceExtend scope

**Question.** Which field of a `SurfaceExtend` scope stores the extension method, and which records can the trailing reference of a SurfaceExtend or boundary-carrier SurfaceOffset group name?

**Known.** `f3d.md` §3.1 "A `SurfaceExtend` scope has at least three ordered references" defines the distance owner and the boundary carrier. The boundary carrier is a counted construction-operand group with role `0x0000000800000000`, `trailing_count` 1, and one trailing reference. The candidate fields are the boolean of a compact flag record, the group `variant` and closing flag, and the u32 fields that precede the scope's marked references to the distance owner and the boundary carrier. Each candidate holds one value in every available scope. A compact flag record has an indexed header followed by ten zero bytes, marker byte `01`, a boolean byte, and terminator byte `00`.

**Need.** Three saves that extend the same edge by the same distance, one with each extension method, compared byte for byte. Controlled saves and record comparisons that establish the trailing reference target forms and their effect on the operation.

**Conflict.** None.

**Note.** The decoder projects each `SurfaceExtend` scope with `SurfaceExtension::Unresolved` and reports `feature.surface-extend-method-unresolved`. It retains each boundary operation and reports `feature.surface-boundary-flag-record-assumed` when its trailing reference does not name a compact flag record.

### DR-312. Negative SurfaceExtend distance

**Question.** What operation does a negative `SurfaceExtend` extension distance select?

**Known.** The distance owner's evaluated scalar is a finite signed value in centimetres. The linked parameter's expression states the same signed value, for example `-0.2 mm`. The neutral `ExtendSurface` distance is positive.

**Need.** Two saves that extend the same edge by a positive distance and by its negation, with the resulting body geometry of each.

**Conflict.** None.

**Note.** The decoder keeps the signed distance in the native operation, keeps the scope native, and reports `feature.surface-extend-distance-nonpositive`.

### DR-323. Suppression columns that name no feature

**Question.** Which feature does a suppression column of a configuration table name when its target resolves to no neutral feature?

**Known.** `f3d.md` §3.1 gives the table's rows and columns. A feature suppressed in the current model with no resolved column gets a suppressed state in the active configuration. When the active row also has a suppression column with no resolved feature, that column could name the same feature with another state. The decoder keeps the inferred state and reports `configuration.suppression-inferred`.

**Need.** Saves of one design with a suppression column on a feature kind that has no neutral feature, in each active row, with the column target and the feature state recorded.

**Conflict.** None.

**Note.** None.


### DR-320. Repeated sketch ordinals in one collection

**Question.** What does a sketch Geometry-member ordinal denote when two sketches of one ordinal collection store the same value?

**Known.** `f3d.md` §3.1 gives the ordinal at Geometry-member offset 30 and the collection reference at offset 37. The counted run of the collection lists both sketches, and each sketch names the collection in return. Entity suffix order and collection reference order do not follow ordinal order, and the sequence skips values. Both sketches of a repeated pair are complete sketches with their own entity header, placement, and Sketch scope. The decoder retains the ordinal and uses it for no neutral field. The structural check admits a repeated ordinal.

**Need.** Controlled saves that create, delete, copy, and reorder sketches in one component, each recording the ordinal of every sketch in the collection.

**Conflict.** None.

**Note.** None.


### DR-321. Fillet scope without a stored radius

**Question.** Where does a `Fillet` scope store its radius when it has no parameter owner and no fixed scalar lane?

**Known.** The scopes that store radii use these forms. Within a scope of the Fillet family, construction-operand groups in scope-reference order pair one-to-one with `Radius` parameters in increasing owner-local order. `TangencyWeight` parameters, when present, pair with the same groups in increasing owner-local order. The group, owner, and parameter records store no radius or tangency-weight assignment cross-reference. The two orderings define both pairings. Each radius therefore applies to the ordered edge operands carried by its paired group. When the scope also holds role-`0x0000001000000000` face groups, the role-`0x0000000800000000` edge groups pair one-to-one with the radii in that order, and a face group in such a scope takes no pairing. `f3d.md` §3.1 gives the fixed `Fillet` scalar lanes in the scope reference table. A `Fillet` scope exists whose reference table holds one role-`0x0000001000000000` construction-operand group and the eight members of that group, with no other reference. The members are compact persistent-identity frames retained in the edge-identity arena. The scope has no indexed parameter owner, no fixed scalar lane, and no radius, chord length, or tangency weight record. The frame of the scope is 22 bytes shorter than the frame of a `Fillet` scope with owners and nine references. Its opaque scalar stored after the feature name is independent of the radii of the `Fillet` scopes that have owners.

**Need.** The same selection saved as a `Fillet` with a known radius, and as a suppressed `Fillet` after the radius is deleted or the feature is suppressed, each with the scope frame, the reference table, and the timeline suppression state recorded.

**Conflict.** None.

**Note.** None.

### DR-322. Selected-face Extrude start without a start group

**Question.** What does start-support byte `2` mean for an `Extrude` scope that stores no start-face group?

**Known.** `f3d.md` §3.1 gives start-support byte `2` as a selected-face start that stores a `ProfileOffset` and a profile-from-entity reference. A selected-face start adds one role-`0x0000001100000000` start group; a role-`0x0000000500000000` group is a start-face group only when the start support is selected face. A new-body `Extrude` scope exists with start-support byte `2`, one-sided distance extent, and a `ProfileOffset` owner, whose reference table holds three fixed scalar lanes, one further record, the profile group, and its profile. It stores no start group of any role. Its reference slots and frame length are those of an offset-profile-plane new-body scope with the same reference count. Selected-face start scopes of other operations store the start group. A one-sided to-face `Extrude` scope with start-support byte `2` stores one role-`0x0000001100000000` face group, which its seventh reference slot names, and a `ProfileOffset` reference in its first slot. When the seventh reference slot names the single face group, the decoder reads that group as the termination and the start as the offset profile plane, and reports `feature.extrude-start-face-absent-assumed`. A single face group that the seventh slot does not name is the start group.

**Need.** Saves of a new-body `Extrude` that start from a selected face, from an offset plane, and from a selected face that is later deleted, each with the start-support byte and the scope reference table recorded.

**Conflict.** None.

**Note.** None.

### DR-326. Extrude start face on another component's body

**Question.** Which identity in a role-`0x0000001100000000` direct start group names the selected face when that face belongs to a body of another component, and in which ASM history and state is it resolved?

**Known.** The group uses the 143-byte direct construction-group frame of `f3d.md` §3.1. Its sole member is a bounded-face recipe operand, and its two auxiliary records each carry one u64 entity identity. The recipe's references resolve no face in the input state of the Extrude's own history. The two auxiliary identities also occur in a second history, as a surface and a pcurve. The surface carries one planar face, and the coedge of the pcurve lies on a different face. In the Extrude's own history, both identities occur as coedges, first inserted after the Extrude's state. Each auxiliary identity occurs in both histories. No single history holds both identities on one face. Both auxiliary records use the compact placement form with variant byte `1`; neither carries a rigid transform. The decoder keeps the start face native.

**Need.** Designs with two components, A placed once in B. B's first `Extrude` starts from a planar face of a body in A and extends a sketch profile 5 mm. Save one design per selected face of A: the top face, a side face, and a face that a later feature of A changes. Save a further design where A is placed twice and the face of one occurrence is selected. Each save needs the start group frame, its auxiliary records, the ASM histories of both components, and the occurrence transforms of A in B.

**Conflict.** `f3d.md` §3.1 names the sole member of a 143-byte direct construction-group frame a persistent entity-selection operand. In this group it is a bounded-face recipe operand, and the decoder reads it as a face operand.

**Note.** None.

### DR-327. Separator byte and wrapper identity of a 130-byte circular-pattern axis record

**Question.** Does the `u8 1` that a 130-byte two-wrapper circular-pattern axis record stores between its references to the axis-record indexes plus two and plus one encode the direction of travel? Which topology does the persistent identity of its wrappers name?

**Known.** The 129-byte two-wrapper form stores two zero bytes there, and the 130-byte form stores `u8 1` and two zero bytes, with every later field one byte later. The 220-byte rectangular-pattern direction record stores the same `u8 1` and two zero bytes between the same two references. The decoder projects a pattern whose axis comes from the 130-byte form only when its total angle is a full turn, where the instance set does not depend on the direction of travel. In a 130-byte record whose pattern repeats an Extrude twenty times over a full turn, both wrappers carry the same persistent identity. No ASM history topology holds that identity. The design type table lists it among the entity identities of the body-visibility browser-node member type `D26351F0-5940-4D23-AA20-2C35475A6D9E`. It is one less than the record index of the seed Extrude's scope. The axis stays unresolved.

**Need.** Saves of one circular pattern of a body about an edge axis, with a total angle of 90 degrees and three instances, once in each direction of travel, plus the same pattern as a full turn. Save the full-turn pattern once about a linear edge, once about a circular edge, and once about a construction axis. Each save needs the axis record frame, its wrapper records, the parameter owners, and the ASM history.

**Conflict.** None.

**Note.** None.

### DR-328. Extrude auxiliary and surface selections

**Question.** What geometry and ordering does a role-`0x0000004300000000` group select in a surface Extrude, and what does a role-`0x0000000500000000` group select in a distance and through-all Extrude?

**Known.** A surface Extrude scope can reference one such group with multiple members. The decoder retains the group and reads it as the profile with `feature.extrude-surface-profile-assumed`. A distance and through-all scope can carry a role-`0x0000000500000000` group; its selection stays native with `feature.extrude-auxiliary-selection-native`.

**Need.** Controlled surface Extrudes that vary the selected curves and their ordering, and distance and through-all Extrudes that vary the auxiliary selection.

**Conflict.** None.

**Note.** The group and its member references remain in the native model.

### DR-329. Extrude second-side face control

**Question.** What does the byte after the second-side extent of a distance-to-face Extrude control?

**Known.** A two-sided distance-to-face extent stores byte `0` or `1` after its second-side extent. The decoder retains the byte in the native extent, reads byte `1` with the face termination of byte `0`, and reports `feature.extrude-face-control-assumed`.

**Need.** Distance-to-face Extrudes that differ only in the second-side control.

**Conflict.** None.

**Note.** None.

### DR-333. Edge recipes whose prefix record index names no tagged face

**Question.** Which faces does a standard Fillet or Chamfer edge recipe select as candidate faces when the i32 record index at `name−16` of its prefix frame is `1`?

**Known.** An edge operand's candidate faces are the faces of the bound ASM history whose persistent subentity tags name the recipe's prefix record index. One public printer assembly stores 1,235 edge operands. 86 of them store record index `1`, and in each of these the bound history holds no persistent subentity tag that names design reference `1`, so the operand has no candidate faces. Four of the 86 store no prefix references. For the other 82, the prefix references resolve to between 4 and 44 faces. Of the 1,149 operands with another record index, 1,039 have candidate faces, but no fixed relation joins those faces to the faces of the prefix references. In 820 of the 1,039, the candidate faces are equal to neither the union of the reference faces nor the union of the faces tagged by the positive references, and are not a subset of the reference faces. The decoder keeps the edges of the 86 operands native.

**Need.** A single-body part whose `Fillet` rounds one edge of a box, saved once. Then the same `Fillet` with a second edge added, and the same `Fillet` after an earlier feature that changes one face next to the edge. Save each design in the top-level component and once in a component placed in another design. Each save needs the edge-recipe operand records, the parameter scope, and the ASM history.

**Conflict.** None.

**Note.** None.

### DR-336. Placement-owner As-built scope with an 87-byte tail

**Question.** How does a placement-owner As-built scope with an 87-byte tail bind its connector frames, motion, and limits?

**Known.** One public wiring assembly stores scope 7461, class pair 371/263, type `AA8B3D40-B67D-4AF1-B3CF-E1E00279B5F0` version 1, with a 421-byte frame. Its eleven reference-table members are 7471, 7473, 7476, 7487, 7490, 7501, 7504, 7507, 7510, 7518, and 7521. Owners 7501 through 7510 supply three zero offsets and a zero angle. Owners 7518 and 7521 supply −π/6 and zero radians. The prefix names locators 7462 and 7465, each of type `2E3AC990-483D-4A55-8662-C064B976A957` version 3. Both locators store a null construction reference and a matrix. Construction 7471 has type `7B6C3C2D-4096-4E6D-B225-699E8C2D9355` version 8 and stores a decoded placement matrix. Motion record 7531 references occurrence records 279 and 4716 and joint record 7460. It stores freedom code 2. The motion decoder binds it to record 279 through its reference at offset 24. With the decoded occurrence placements, both locator matrices compose to the same frame in the occurrences' shared parent within a maximum matrix-cell difference of 2.5e-14 after translation-unit conversion.

**Need.** Define the roles of the motion record's ordered references and the admission rule that distinguishes the placement-owner layout from the direct-construction layout. Establish the motion and limit association for this layout.

**Conflict.** The direct-construction 421-byte layout places scalar owners at reference positions four through seven and its solved frame at position eight. This scope places scalar owners at positions five through eight and stores a selection record at position four. Frame length selects two distinct layouts.

**Note.** The decoder retains the scope and its six parameter values. Neutral projection requires the placement-owner layout and its motion binding together.

### DR-338. Extended circular-pattern axis controls and selection runs

**Question.** How do the controls and selection runs of the 220-, 221-, and 241-byte circular-pattern axis frames determine the directed axis and its selected entity?

**Known.** The decoder admits the 195-byte inline axis frame, the 221-byte frame with selection kind `8` as an inline axis, and the 118-, 129-, and 130-byte identity forms. An extended frame carries an origin and displacement in the same positions as the rectangular-pattern direction frame. Three body instances over a quarter turn about a 221-byte frame with a Z displacement lie at 0, 45, and 90 degrees counterclockwise about that displacement. Two 221-byte component-pattern frames carry a unit X displacement and an origin at Z = 1.92 cm. Their saved occurrence transforms are positive quarter turns about that line. A 220-byte feature-pattern frame carries a Z displacement. Its seed holes occur in symmetric pairs, so positive and negative quarter turns yield the same saved hole set. A 241-byte body-pattern frame carries an X displacement and three consecutive selection references. Its total-angle owner stores a full turn.

**Need.** Bind each extended selection run to its selected entity. Compare the controls for three instances over a partial turn in each travel direction, with asymmetric seeds, for each frame form. Bind each instance to its saved BREP geometry.

**Conflict.** None.

**Note.** DR-327 tracks the separator of the earlier two-wrapper form.

### DR-339. Circular component-pattern seed paths

**Question.** How does a circular pattern's role `0x0000010000000000` path group identify its seed occurrence and ordered generated occurrences?

**Known.** Two circular component-pattern scopes have an empty member run in this role and two auxiliary paths. Each scope has one later ordinal-2 occurrence carrier and an earlier ordinal-1 carrier with the same component GUID. Both generated carriers store the positive quarter-turn transform described in DR-338. The decoder's component-pattern occurrence binder consumes rectangular-pattern instance frames. Circular-pattern seed selection requires a nonempty face or body group.

**Need.** Join the auxiliary path identities to occurrence carriers through stored references. Confirm the join with multiple earlier occurrences of the same component and with reordered generated occurrences. Bind the selected occurrences to saved instance geometry.

**Conflict.** None.

**Note.** None.

### DR-341. Chamfer distance reference face

**Question.** Which stored field assigns the first distance of a two-distance chamfer, or the distance of a distance-angle chamfer, to an input support face?

**Known.** Chamfer parameter owners supply ordered dimensional specifications. Edge recipes retain ordered sides and prefix-reference ordinals. Exact ASM transitions retain support-face carriers and the inserted treatment carriers. The projector transfers the dimensional specification and the edge selection into `ChamferGroup`. That IR type carries the selection and specification. The projector sets `flip_direction` to false.

**Need.** A rule that binds each selected input edge to its distance reference face, with both face widths checked against the stored dimensional specification. The rule must cover reversed recipe-reference order, propagated edge chains, and support faces split by earlier operations.

**Conflict.** None.

**Note.** The neutral representation needs a reference-face binding for each selected edge before it can retain a proven assignment.

### DR-342. Chamfer recipes with conflicting transition selections

**Question.** Which stored context selects the input edges when a standard chamfer recipe and the exact ASM treatment transition supply different edge candidates?

**Known.** A standard edge recipe can retain several primary-reference boundary candidates or an empty intersection. The same scope can carry a compact persistent identity with a nonempty treatment-transition chain. A chain describes the complete operation transition. The projector retains independently resolved recipe edges in a partial historical selection and retains the unresolved operand identities.

**Need.** A rule for the recipe's evaluation context and selected edge run that accounts for every operand and every edge of the treatment transition. A multi-member group also needs the assignment between members and selected edges when the recipe carries distinct selections.

**Conflict.** None.

**Note.** DR-333 tracks the candidate-face meaning of prefix record index `1`.

### DR-343. Chamfer rotation parameter on oblique support faces

**Question.** How does `rotateAngle` determine the second setback of a chamfer whose input support faces meet at an oblique angle?

**Known.** The projector pairs `leftDistance` with `rotateAngle` and transfers the pair as `ChamferSpec::DistanceAngle`. That IR form measures its angle from the distance reference face. An operation with a 0.4 mm `leftDistance` and a 30-degree `rotateAngle` has input plane normals `(0, 0, 1)` and `(0.8660254037844384, 0, 0.5000000000000004)`. Its result treatment plane has normal `(0.6265218814381287, 0, 0.779403831193578)`. The stored distance is the setback on the second input plane. The other setback is approximately 0.230940108 mm.

**Need.** A dimension rule checked on acute, right-angle, and obtuse support pairs with both setback assignments. The checks must distinguish a face angle from a parameter that encodes the ratio of two setbacks.

**Conflict.** The result plane forms an angle of approximately 21.2 degrees with the second input plane and 38.8 degrees with the first. Each differs from the 30-degree IR angle.

**Note.** DR-341 tracks the input reference-face assignment.

### DR-360. One-sided face-target Extrude without a termination group

**Question.** Which stored relation supplies the termination face of a one-sided face-target Extrude whose operand roster has no termination group?

**Known.** A one-sided face target (direction `1`, first-side extent `2`) stores one role-`0x0000001100000000` termination group, distinct from its start group. A selected-face start adds one start group before it in scope-reference order. A shape target replaces the termination group with one role-`0x0000000500000000` target group.

**Need.** The termination face of such a scope, or the stored state that makes the extent end without one.

**Conflict.** The Mini AfterSherpa V6 Zodiac BMO part stores a one-sided Cut scope with a selected-face start, one start face group, a `Side1Offset` owner, no termination group, and no shape-target group.

**Note.** The Conflict scope has the DR-322 form: its seventh reference slot names its single face group.

### DR-361. Treated edges in executed treatment transitions

**Question.** Does an executed `Fillet` or `Chamfer` transition delete every treated edge?

**Known.** The two-face group rule assumes that the transition deletes every treated edge and that the treated edge set equals the transition-deleted set. The rule checks that the deleted-set size equals the member count, each member has two distinct preceding faces, each member's shared changed-edge candidates are nonempty and belong to the deleted set, the candidates cover that set, a perfect assignment pairs each member with a distinct candidate, and each resolved member edge belongs to the deleted set. A group selected by this rule charges `history.edge-selection-deleted-set-assumed`.

**Need.** A treatment whose treated edge survives the transition, or proof that every treated edge is deleted.

**Conflict.** None.

**Note.** None.

### DR-376. Stored name of a Sketch renamed in the timeline

**Question.** Where does a Sketch renamed in the timeline store its name?

**Known.** The placement reader in `design/decode/sketch.rs` reads the display-name slot in its empty form. The empty form occurs in all ten Sketch placements checked in two public inputs: nine in the Micron motor cover and one in ParametricLogo. The projected Feature name joins the stored scope kind spelling, a space, and the feature ordinal. A planar or spatial Sketch takes this name when one placement in the same stream carries its scope, one Feature with a non-empty name projects from that scope, and one scope selects the sketch.

**Need.** A design with a Sketch renamed in the timeline.

**Conflict.** None.

**Note.** The decoder charges one aggregate `sketch.name-composed-from-feature-ordinal` loss when sketches take the composed name.

### DR-340. Compact profile loop groups and their region descendants

**Question.** Which regions of a compact profile loop group form the feature selection when the group carries an initial region and further regions that divide it?

**Known.** The HexGrid Extrude 3 profile operand names Sketch entity 245. Its nested record 7385 starts with a reference to profile operand 7382 and a counted loop recipe. The first region has exterior curve identities 276, 274, 275, 277 and hole identities 120:114, 122:118, 123:119, 121:116. Two further regions divide this rectangular annulus at the rectangle with curve identities 105 through 108. Each entry has kind 3, a primary and secondary identity, three operands, and eight zero bytes. Every listed identity binds to a curve owned by Sketch 245. The neutral Sketch carries two profile loops: the inner rectangle with primary identities 120 through 123 and the central hexagon with primary identities 131 through 136. The outer rectangles' curves stay outside those two loops. The decoder retains the compact loop groups in `design_profile_curve_selections`. Its complete-loop projection requires an exact loop join and a complete analytic Cut-transition certificate. In public `Parametric_HexGrid_Generator v1.f3d`, scope record 5927 starts at Design BulkStream offset 97111, profile operand 7382 starts at 114942, and its identity record starts at 115298. Construction group 7379 owns the profile operand. Persistent selection group 5948 at offset 98256 also belongs to the scope; its member 9569 at offset 141041 carries local identity 4696. The compact curve join requires an empty further-region list. All offsets refer to `FusionAssetName[Active]/Design1/BulkStream.dat`. Rectangle diagonals 278 and 279 use the construction-tail frame tracked in DR-463.

**Need.** A selection rule for the initial region and its further regions, with cases that select different proper subsets of the descendants. Bind the three entry operands to direction and curve portions for the compact spelling.

**Conflict.** None.

**Note.** Extrude 3 retains its native selection group. Existing `SketchRegions` with explicit boundary uses can represent the rectangular annulus after its selected boundaries and traversal directions are established.

### DR-358. Compact complete-loop region identity across sketch arrangements

**Question.** Which stored relation selects an atomic region when a compact complete-loop group names boundaries that occur in several nested or intersecting regions?

**Known.** The compact group lists exterior and hole loops through persistent curve identities. An exact neutral loop join identifies each boundary. A one-sided blind Cut from the profile plane can certify its candidate set through the changed ASM planar regions and analytic lateral carriers. The decoder retains the native selection when this certificate is incomplete. A scope can have an empty face delta or an unbound history state.

**Need.** The relation between the compact entry operands, the arrangement region identity, and the selected side of each boundary, including intersecting loops, nested loops, selected-face starts, and operations with an empty face delta.

**Conflict.** None.

**Note.** DR-340 covers the further-region program. This item covers groups whose further-region counts are zero.

### DR-335. Extrude bounded-face programs across boundary changes

**Question.** Which stored relations bind an `Extrude` start or termination recipe when its node run covers an earlier boundary, a node has one side clause, or a prefix operand has an empty face lane in the preceding topology?

**Known.** A two-sided termination stores two bounded-face recipes with Design reference `302`. One program has five nodes, of which four have a parsed two-clause structure. The other has four parsed two-clause nodes; its first prefix operand names face slot `50`, whose preceding loop has eight edges. Its aggregate candidate lane contains four side faces. A further termination program has eight parsed nodes and two empty prefix-face lanes. Two further programs have five nodes each, including a node with one side clause (`f3d.md` §3.1). Their remaining candidates include several preceding faces.

**Need.** The mapping of a saved boundary edge to a chain of preceding edges, the edge that a one-clause node binds (DR-170), and the binding of empty prefix-face lanes. Independent selections before and after a boundary split must distinguish a selected face from its adjacent faces.

**Conflict.** None.

**Note.** These start and termination groups retain their native identities when the candidate, support, complete-loop, and plane proofs remain unresolved.

The compact Sketch-point termination of Extrude 4 in `M3 Parametric SHCS.f3d` is tracked in DR-480.

### DR-359. Region selections with curve identities absent from their Sketch

**Question.** When a counted region selection names a curve identity that its Sketch no longer owns, do the atomic regions that the stored Cut transition removes equal the selected regions?

**Known.** A later Sketch edit can remove a curve identity that a region selection names. The reading takes the atomic arrangement regions that lose material on a changed plane parallel to the Sketch. The candidate set must pass the compact Cut certificate, its region count must equal the stored region count, and every stored member identity that the Sketch still owns must bound the corresponding region. A feature read this way charges `feature.profile-region-identities-stale`.

**Need.** The stored relation between a removed curve identity and its replacement curves, or a Cut whose stored transition removes a region outside its selection.

**Conflict.** None.

**Note.** DR-358 covers compact selections whose curve identities all resolve.

### DR-357. Compact owned Thread option semantics

**Question.** Which compact owned Thread field states the modeled flag? What property does the prefix word encode, and what construction option does the byte `C` encode beyond its owner-reference order?

**Known.** The version-four class-`326/263` compact owned frame has a zero u32 at primary-relative offset 33. The compact marker starts at offset 37, the `6H` token at offset 42, and the designation at offset 46. Construction marker `00 01 00 00 00` has leading offset and length owners; marker `00 02 00 00 00` has trailing offset and length owners. Both trailers repeat the length and offset owner references. Both pitch markers are zero. The selected internal cylinders have the stored minor diameters. The decoder retains these forms and their owner order; the neutral modeled state remains unspecified.

**Need.** Controlled changes to the prefix word and construction option, with complete scope bytes, face orientation, and resulting geometry.

**Conflict.** None.

**Note.** DR-142 covers construction options. DR-205 covers modeled extended-prefix entries.

### DR-334. Persistent tag token integer meaning

**Question.** What does the u32 integer after a Design recipe-prefix token and between a B-rep generic-tag token and its reference count mean?

**Known.** The pair (token, token integer) identifies the persistent tag group. The decoder retains the integer in generic-tag groups and both standard-prefix token frames. A Design reference joins a tag with the same pair and Design reference. Most groups store integer `0`. In 70 public Design streams, 154 edge-recipe prefix fields store a nonzero integer, with values `4012`, `6798`, and `12173` after tokens `3273`, `726`, and `3666`. Each of 157 nonzero recipe fields in one part has a live generic-tag group with the same pair. The compared body-map suffixes, history-state identities, reference counts, and byte lengths differ from the integer. In one part, 30 edge-recipe occurrences of `4012` equal an indexed record index and a Design type-table entity ID; in another part, 95 occurrences differ from both. Voron 1.8 A/B Drives stores `(5, 82902527, -300)` and `(5, 1773368033, -300)` in the Extrude 5 prefix and on input face `40551` in state `3672`. The latter operand is unframed. Extrude 16 stores token `10` with integers `1437049394` and `1888070796`, each with references `-300`, `328`, `329`, `-372`, and `-373`; input face `48809` in state `3725` carries both groups. These complete prefixes resolve to faces `40551` and `48809`. Main selected cylindrical faces `40553` and `48872` after rejecting the prefixes and the nonzero tag groups. The intermediate reader retained the tag groups and rejected the unframed nonzero prefix operands, leaving both selections native. For token `10` with references `328` and `329`, equal-integer matching selects face `48809`; token/reference matching adds faces `19507`, `25284`, `29882`, and `50093` with integer zero. All other references in the two prefixes have the same live candidates under both joins. The disjoint counted-name operands in DR-462 use integer zero in every name.

**Need.** Determine whether equality of the retained integer is an identity invariant across revisions and how to frame an unframed token when several token lengths give valid integer/count/reference fields. Compare paired saves with the same persistent edge selection before and after an operation that gives a tag group a nonzero integer, for example a Fillet on an edge of a body that a Combine or a Split later changes. Each save needs the recipe prefix and the generic-tag groups of the BREP history. Resolve the counted-name candidate composition in DR-462 independently of integer equality.

**Conflict.** None.

**Note.** Zero and nonzero integers use the same pair comparison.


### DR-462. Counted persistent-name candidate composition

**Question.** How do the persistent names of one counted recipe operand constrain the selected face fragments when their live candidate sets overlap or are disjoint?

**Known.** Fillet 22's 29 members and Chamfer 12's 12 members in Voron 1.8 A/B Drives carry counted operands with several persistent names. Intersecting the live name sets resolves one edge per member; transitions `3769 → 3770` and `3866 → 3867` delete every resolved edge. Name union admits extra face fragments through `(2, 0, -420)`, `(2, 0, -421)`, `(1, 0, 328)`, and `(1, 0, 329)`, which makes singleton edge intersections ambiguous. In New XY Joints M3x8mm Chamfer, Fillet 20 member `206364` has a counted operand with `(1, 0, 530)` on faces `43973` and `48057` and `(7, 0, 375)` on face `49019`. Its other primary-side operand names face `49001` through `(6, 0, 375)`. The first operand's name sets are disjoint. Their union and the other operand share edge `49028` between faces `49019` and `49001`; the Fillet transition from state `9016` deletes that edge. In Front_Idler_Body_AB, Chamfer 2 from state `246` preserves main's selected edge `4713` and deletes all fourteen edges selected with the common-name constraint.

**Need.** Determine the relation between name order, historical name validity, and face fragmentation. Compare paired saves with the same selected edge before and after a topology change that makes the live name sets disjoint. Retain the counted prefix, the input and result generic-tag groups, and the complete edge transition.

**Conflict.** The common-name constraint removes aliases in A/B Drives and removes a transition-deleted edge in New XY Joints M3x8mm Chamfer.

**Note.** The decoder uses the nonempty name intersection. For disjoint live name sets it retains their union and requires a unique transition-deleted common boundary edge for the fragment fallback. The inferred-selection loss records this assumption.

### DR-362. Non-outer loops in Loft section selections

**Question.** What do the non-outer loops of a Loft section's stored region selection select?

**Known.** The Micron EZ motor cover v3 design has Loft 7 in scope record 3516. Section group 3518 on Sketch @309 lists its six-curve outer loop and its nested ten-curve non-outer loop. Section group 3526 on Sketch @172 lists outer loop A and nested non-outer loop B. The stored body in state 277 has one lofted side face and two filled planar caps. Each cap has one loop. The lower cap follows the complete outer loop A of Sketch @172; the upper cap follows Sketch @309's six-curve outer loop. The DR-362 public-file scan covered 150 F3D files below 20 MB: 145 decodes completed and five reached the 120-second limit. The completed decodes contain five Lofts across three independent designs and twelve Revolves. The classified non-outer-loop selections occur in two sections of one Loft in one independent design. The scan found zero confirmed outer-only selections that contain another loop of the same Sketch. Native section groups and counted spatial-profile member lists remain selection-coverage limits.

**Need.** A Loft whose stored body has a hole in a section cap, or a Loft whose stored section lists only an outer loop while that loop contains another loop of the same Sketch. Compare the stored selection flags with the cap topology.

**Conflict.** The Extrude reading of the same record form uses the exterior flag to select the exterior loop and the other loops to select holes. That reading selects rings for both Loft 7 sections, while the stored caps have filled areas.

**Note.** The Loft section binder retains selections with non-outer loops as native profiles and records `feature.profile-selection-native`.

### DR-461. One-sided face-target Extrude direction and offset reference

**Question.** Which vector defines the signed `Side1Offset` of a one-sided face-target Extrude, and how is travel selected when the target lies behind the start plane?

**Known.** The Runice GT2 20T B5 9mm design stores Extrude record `1424` with profile normal `+Z`, profile offset `0.75` cm, direction `1`, direction reversal `0`, and `Side1Offset = -0.15` cm. Its ASM transition from state `6` to state `22` inserts plane face `709` at z=7.5 mm, plane face `394` at z=18.5 mm, and 120 cylindrical side faces with boundary z coordinates from 7.5 to 18.5 mm. The input state has planar annular faces `3465` at z=0 and `3484` at z=20 mm. The termination operand carries both faces among its recipe candidates. The decoder projects face `3465` as the historical termination. The rear vertex upper brace design stores Extrude record `4140` with profile normal `+Z`, target face `4051` at z=8 mm, and `Side1Offset = 1.6` cm; its transition inserts plane face `4744` at z=24 mm. The travel, profile normal, and target surface normal are parallel in that case.

**Need.** A termination-recipe rule that selects the target from these candidates, and independent transitions with a nonzero offset whose profile normal, oriented travel, and target surface normal distinguish the possible sign rules. A target behind the start plane must also distinguish the direction-reversal setting from travel toward the target.

**Conflict.** The projected z=0 target and a signed displacement of magnitude 1.5 mm give termination planes at z=-1.5 or z=+1.5 mm. The transition terminates at z=18.5 mm. The z=20 candidate gives that plane with a displacement of -1.5 mm along `+Z`.

**Note.** The transition travels along `+Z` and agrees with direction reversal `0`. The decoder source property `extrude_to_distance = 7.5` equals the start-plane offset in this transition.

### DR-465. Matrix Scale prologue word

**Question.** What does a nonzero matrix Scale prologue word control?

**Known.** The matrix supplies positive axis factors and translation consistent with its point-data center.

**Need.** Paired saves that change the word and preserve the factors and center.

**Note.** The decoder applies the stored matrix and records `feature.scale-matrix-word-assumed` for a nonzero word.

### DR-464. Matrix Scale body groups outside the prologue

**Question.** How do owned body groups absent from the matrix prologue contribute to the operation?

**Known.** The prologue lists the body groups for the stored transform. A scope can own additional body groups with the same body-selection role.

**Need.** Paired saves that vary the listed and additional selections independently.

**Note.** The decoder selects the listed groups and assumes that the additional groups select the same bodies. It records `feature.scale-unlisted-groups-assumed`.

### DR-469. DerivedInstance sources without a local occurrence join

**Question.** How does a `DerivedInstance` scope identify its inserted components when its ordered references do not give a unique local relation and occurrence carrier?

**Known.** The canonical scope kind identifies the derivation family. Its ordered reference run retains the source record indices. A local three-reference relation links an occurrence carrier to its owning scope. Source membership and placement can remain unresolved when this join fails.

**Need.** Identify the construction fields and definition references of the other scope generations and of references whose target records are absent.

**Conflict.** None.

**Note.** An unresolved scope keeps a native component selection inside a neutral component insertion and reports `feature.derived-instance-component-insert-assumed`. Its native scope reference and ordered reference properties remain present.

### DR-466. Standalone joint source operand binding

**Question.** Which document identity namespace and occurrence context do qualified source paths select when their keys do not match admitted occurrence carriers?

**Known.** The qualified assembly scope carries connector frames and alignment values. Its occurrence path carries the source document identity, occurrence key, and ordered descendant keys. These fields describe a readable joint independently of availability of the referenced document.

**Need.** Public document bundles with independently identified source documents and repeated component instances that bind each source path identity to its document and occurrence context.

**Note.** A standalone timeline feature names its decoded neutral joint while source operands remain unresolved. The decoder preserves their identities and charges `assembly.joint-source-binding-assumed`.

### DR-485. Missing Decal image definitions

**Question.** Which source image and target does a Decal scope select when its image definition is absent or has an unparsed frame?

**Known.** The Decal scope identifies a presentation operation. Its prefix stores the image-asset reference, mapping byte, and target-group reference. The referenced image definition supplies the image basename.

**Need.** A save that preserves the selected image and target while its definition uses another framing form.

**Note.** The decoder retains the scope identity as an external image source and records `appearance.decal-definition-unresolved`. It preserves parsed prefix values in source properties and uses the target-group reference to select faces. Missing target bindings produce `appearance.decal-target-unresolved`. An absent mapping byte or a byte other than `0x60` produces `appearance.decal-mapping-assumed`.

### DR-476. Controls of the wider SurfaceDeleteFace prologue

**Question.** Which settings do the controls before the ordered-reference count of the wide surface-delete prologue encode?

**Known.** `f3d.md` §3.1 "Unhealed surface-face removal" defines the ordered-reference partition. The scope names one role-`0x10` face group followed by its ordered members. Its fixed tail uses the common parameter-scope grammar. The wider prologue precedes the same reference partition.

**Need.** Saves that change one face-deletion setting at a time and identify the corresponding prologue control.

**Note.** The decoder reads the group as an unhealed face deletion and records `feature.surface-delete-face-wide-prologue-assumed`.

### DR-477. Additional face-deletion controls and incomplete group definitions

**Question.** Which controls in further face-deletion frames change the selected group or removal mode, and which face selections do incomplete group definitions name?

**Known.** `f3d.md` §3.1 "A `DeleteFace` scope has at least two ordered references" defines the face group and healing mode. The `SurfaceDeleteFace` family removes faces without healing. The scope retains its ordered references and source identity when its group cannot be bound.

**Need.** Controlled saves that vary further frame controls and complete native definitions for the referenced face groups.

**Note.** The decoder preserves the neutral deletion family. It records `feature.delete-face-frame-controls-assumed` for unfamiliar frames and `feature.delete-face-group-selection-assumed` for missing or inconsistent face groups.

### DR-480. Extrude termination through a compact Sketch-point identity

**Question.** How does an Extrude termination group use a compact Sketch-point selection to set its travel, including an offset and a two-sided extent?

**Known.** In `M3 Parametric SHCS.f3d`, Extrude 4 scope `4751` owns termination group `4766` at Design byte `68879`. Its member `4769` starts at byte `69143`. Identity record `4772` starts at byte `69584` and ends at byte `69621`. It stores ten zero bytes after its indexed header, then u64 values `4125` and `128` at bytes `69605` and `69613`. This is the compact Sketch-point layout in `f3d.md` §3.1. Sketch `4125` owns point record `4168`, persistent identity `128`, at byte `32753`. Its stored coordinates at byte `32894` are `(-0.125, -0.13, 0)` cm. Placement record `4137` maps this point to `(0, -1.25, -1.3)` mm. The profile Sketch `4693` has an identity placement. The final socket floor uses plane surface `49` at `z = -1.3` mm. The Extrude input topology has ten faces. The final topology splits the floor into six faces on surface `49`. Entity-selection decoding admits this compact identity for Loft point groups; Extrude termination projection consumes face operands and WorkPlane selections.

**Need.** A second independent design with this termination form and known point selection, plus paired point-height, offset, and sidedness changes. Establish the travel plane and the relation that preserves the selected Sketch point when its adjacent faces change.

**Conflict.** The termination group role also carries face targets. A point at the floor height and several coplanar final faces permit different face assignments.

**Note.** The decoder keeps group `4766` as the native termination and reports `feature.face-selection-native` and `feature.definition-incomplete`. DR-125 and DR-335 track counted face recipes.

### DR-481. Chamfer edge selection carried by bounded-face recipes

**Question.** Which edges do two bounded-face members of a Chamfer edge group select, and how do their boundary nodes assign the transition edges to each member?

**Known.** In `Parametric_PTFE_Umbilcal_guide.f3z`, Chamfer 4 scope `4323` changes state `500` to `501`. Group `4324` starts at Design byte `85668`, has role `0x0000001000000000`, and names members `4327` and `4331`. Their compact identity prefixes start at bytes `85962` and `91136` and name local records `4330` and `4334`. Each local record carries `bounded_face_recipe_data`, at bytes `86988` and `92162`, followed by a counted program with twenty parsed nodes. Both operands retain preceding candidate faces `746`, `752`, `754`, and `760`. Their prefix references also name faces `631`, `634`, `748`, `750`, `756`, `758`, `762`, `764`, `769`, `820`, and `327`. References with Design values `324` and `326` have empty candidate lanes. The input topology has fifteen faces and thirty-eight edges. Each compact identity retains the same twenty-two transition edge candidates: `637` through `641`, `643` through `645`, `647` through `652`, `654` through `656`, and `658` through `662`. Each selected-face candidate has a four-edge preceding loop. The group has a 0.4 mm equal-distance Chamfer specification.

**Need.** Independent Chamfer designs with bounded-face members, including one selected face, two adjacent selected faces, and a face whose boundary splits before the operation. Establish the selected boundary, the treatment of tangent boundary edges, and the binding of empty prefix lanes.

**Conflict.** A complete twenty-node program, four face candidates, and a shared transition set permit several member assignments. Selecting every transition candidate assumes a group-level rule.

**Note.** The decoder retains the bounded-face operands and compact identity candidates. Group `4324` stays native with `feature.edge-selection-native`. DR-224 tracks Fillet transition ambiguities; DR-244 tracks treatment groups whose transitions supply no candidate set.

### DR-482. Source binding of timeline-owned Base Feature results

**Question.** Which records bind the source bodies and placement of a timeline-owned `Base Feature` when its stored body runs have a complete result binding?

**Known.** Eight recipe-absent scopes in six public designs have timeline owners. Four owners have paste type `BA3059E4-4B23-4D02-9D18-E7BB95C991F3`; four have body-to-component type `56225BC7-69B7-446A-9AEA-6AA25BC7806E`. The Mini AfterSherpa Mosquito scopes at Design offsets `576266` and `588319` have 324-byte frames and owners `5681` and `5752`. Both owner frames carry a null removal GUID. Their scopes contain parameter-body and result-body runs followed by a counted passive-reference run, shared metadata, and a result-record run. The paste GUID-pair reader stops at the counted passive-reference run and reports `feature.paste-sources-unresolved`. Trident z_carriage_rear scope `132579` at offset `2154855` and z_chain_lower scope `35911` at offset `692895` have 485-byte paste-owned frames. The body-to-component scopes occur in Mini AfterSherpa V6 Zodiac BMO, Molex panel mount parametric, and Trident z_mount_rear. All eight retain resolved stored-body definitions.

**Need.** A source-identity join for the passive-reference form, and a source and destination component join for the body-to-component form. Check the joins on two independent designs and compare the source and result topology, placement, and history transition. A consumed source requires its preceding-state identity.

**Conflict.** None.

**Note.** DR-260 tracks placement of a pasted copy. The result binding establishes the captured bodies. The decoder reports `feature.stored-bodies-recipe-absent` for these eight scopes.

### DR-483. Terminal marker in an ordinary Base Feature count field

**Question.** What does the terminal byte of the marked null field mean when it is `1` before the repeated-reference count in a Base Feature envelope?

**Known.** Mini AfterSherpa Orbiter V2 Test v2 has Base Feature scope record `213` at Design offset `107014`, class pair `329/264`, frame length `318`, and history transition `365` to `404`. Its first two marked fifteen-byte entries name `219` and `225`. Both targets have Body presentation-owner type `D3937028-C20C-4E65-B010-94AD418A5C20`. The eleven-byte field after those entries starts with `1`, has nine zero bytes, and ends with `1`. A u32 count `1` follows, then a marked reference to `219`, shared metadata record `217`, and result record `227`. The ordinary result-body reader requires ten zero bytes after the first marker and stops at the final byte. The history transition binds body `f3d:brep:entity#1` and supplies the resolved Base Feature result set.

**Need.** A rule for the terminal marker and the roles of the two initial reference runs. Compare a controlled capture, paste, and body-to-component operation with equal geometry, including their referenced Body records and preceding ASM state. Confirm the role rule in a second independent design before selecting a source or construction operation.

**Conflict.** None.

**Note.** The specialized scope payload stays uninterpreted. The result bodies remain available with `feature.stored-bodies-recipe-absent`.
