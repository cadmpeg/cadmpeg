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

### DR-1014. Order of a body-override appearance and the body's material assignment

**Question.** When a body has a material assignment that names one appearance and a body-override record that names a different appearance, which appearance does Fusion show on the body?

**Known.** A body-override record takes precedence over the body's material assignment: when the two name different appearances, the body binds the override's appearance. `docs/cadir-v11.md` open item 233 concerns an appearance override on a root component or occurrence, not a body record, and does not decide this order.

**Need.** A save with a body whose material assignment and body-override record name different appearances, with the displayed appearance of the body.

**Note.** The decoder binds the override's appearance, replaces the binding of the material assignment, and charges `appearance.body-override-precedence-assumed`.

### DR-1006. Coordinate System construction inputs and frame ownership

**Question.** Which type registration identifies a timeline Coordinate System, and how do its construction references select the origin, axes, and containing frame?

**Known.** `f3d.md` §3.1 "A `JointOrigin` scope owns one coordinate frame" defines admitted scope matrices and assembly operand frames. The separate placement-carrier grammar follows §3.1 "A version-2 `JointOrigin` scope can store its solved frame". A JointOrigin can have a 333-byte version-2 scope with nine ordered references. Its first reference names a 389-byte version-8 placement carrier with reference type 20 and four value references. The carrier stores a rigid matrix and a scope backlink. The carrier translation can lie outside the model-space z extent of the body: translation (172.435358383, 15.303201895, 38.907575495) millimetres occurs with body end planes at z = 0 and 14 millimetres. The decoder keeps this scope native with `feature.coordinate-system-frame-unresolved`. Admitted solved frames transfer to v11 datum frame caches with an unread construction input. A frame whose scope keeps its source construction references and parameter expressions charges `feature.coordinate-system-construction-native`; a frame whose scope keeps none charges `feature.coordinate-system-construction-dropped`.

**Need.** Bind the type registration, carrier frame, occurrence space, input roles, offset signs, and axis flags to final geometry in independent designs. Establish the relation between the timeline label Coordinate System and the registered JointOrigin type.

### DR-817. Silhouette Split direction selection

**Question.** How does the direction selector bind to the datum or topology in the state before the split?

**Known.** `f3d.md` §3.1 "A version-0 `SilhouetteSplit` scope has four ordered plain local references" identifies the selector in the scope and in the point-direction carrier. The carrier stores a solved position and vector in model space. The direction selector can name an axis, plane, edge, or face.

**Need.** A join from the selector record to the source feature or topology in the preceding state.

### DR-502. Compact profile members and descendant selection

**Question.** Which replacement curve closes a compact profile hole after a curve edit, and which descendant regions of the compact group form the selected material?

**Known.** A compact Extrude selection can have an exterior of eleven line members and four hole rings of one circle member each. One hole member names an indexed record that carries a persistent primary curve identity, and its curve is absent from both the native curve-identity arena and the projected Sketch. The current Sketch holds a circle outside the member list with radius 1.675 millimetres and centre `(53.5, 2.5)` millimetres; its persistent curve link names a BREP coedge. A compact group can have one initial exterior with ten entries and five further regions. Their outer/hole entry counts are 5/0, 12/1, 5/0, 1/0, and 1/1. Every named curve of this group resolves. The initial loop repeats two curves with fragment ordinals one, two, and three. Two short lines have opposing uses on coincident portions of those two curves. DR-340 records the compact descendant-selection question. The direct complete-loop join requires empty further-region lists and incidence counts of one. A complete initial boundary can also resolve across subdivisions when its directed curve intervals equal the boundary of the removed atomic regions and a complete analytic Cut-transition certificate agrees. A second compact group on the same Sketch has a member with two initial five-entry exteriors, and the same two repeated curves each carry fragment ordinal two of three. These compact selections retain native operands.

**Need.** A stored replacement or feature-time revision join for the absent hole circle, and a material-selection rule for the initial region and its descendants. The rule must bind the compact incidence operands to directed curve intervals and identify the selected holes.

### DR-500. Counted profile incidence ordinals and curve partitions

**Question.** Which curve partition does the second incidence word select, and what does the third incidence word count?

**Known.** Each 40-byte member carries a traversal flag and two positive incidence words. A Revolve can select one curve with `(1,2)` and a second curve with `(2,2)`; its directed arrangement boundary uses the first interval of the first curve and the second interval of the second curve. A Revolve can select a curve with `(2,3)`. The complete intersection partition of that curve has four pieces, and the stored last word is three. Its selected-member graph excludes one Sketch curve and has three pieces; the second piece equals the directed arrangement interval. A Revolve can repeat one curve with `(1,2)` and `(2,2)` in one exterior loop. A curve can carry `(3,3)` while its full selected-member curve graph has four pieces at the intersections with three other curves. The construction-path grammar in §3.1 supplies fragment ordinals and counts from a serialized primary/related tracking graph. The existing arrangement resolver derives exact parameter ranges and directions from unique directed boundary walks. A symmetric intersect Extrude can have counted members that each join uniquely to a Sketch curve while other Sketch curves with tail marker zero divide its boundary. The member-ring resolver accepts one atomic arrangement boundary with the complete named-curve ring and traversal directions. It also accepts a boundary whose fragments of one curve meet only at nodes of curves outside the ring, when each member is piece `k` of the `n` pieces into which the other curves of its member list cut its curve; this reading carries `feature.profile-piece-partition-assumed`. Stored members agree with the member-list partition in these cases: pieces are numbered along the curve's parameter direction for both traversal flags; the pieces of a full circle start at angle zero and continue counterclockwise; each of two overlapping lines has its own member, each is cut at the ends of the other, and the part of a line beyond a corner counts as a piece; arc pieces are numbered along the arc; curves outside the member list that end on, cross, or touch a member curve add no cut; the relation holds for curves added to the Sketch after the feature. The transition certificate accepts one-sided blind Cut, Join, and NewBody. The selection retains its native identity with `feature.profile-selection-native`.

**Need.** The join from each counted member to its native primary/related tracking graph, the partition domain and circle phase, the treatment of line extensions, clipped members and coincident curves, and the relation to feature-time sketch revisions. These rules must determine both incidence words and the selected interval in independent designs.

**Note.** The decoder reads the two incidence words as the piece ordinal and count in the member list's own partition. Other member curves cut the curve, and pieces follow its stored direction. The first piece of a circle contains angle zero. The decoder joins consecutive fragments of one curve and direction at nodes shared only with curves outside the list. It selects a joined ring when every member names a distinct piece in that partition and every fragment has one curve. It reports `feature.profile-piece-partition-assumed`.

### DR-501. Profile members after sketch boundary edits

**Question.** How does the live profile tracking carrier resolve a boundary selection after Sketch curve edits?

**Known.** A Revolve operand can have one region with 17 members. Every member resolves to a projected curve. The current sketch has zero stored profiles. An arc outside the member list joins two selected lines, and two tangent relations join this arc to those lines. A Revolve operand can have one region with 21 members and an arc outside the list that closes the gap between two selected lines. A Revolve operand can have six members and a line outside the list that closes the gap between two selected lines. In each of these forms the selected curves alone have open ends. Each affected profile group has a trailing identity wrapper followed by EntityTracking type `50AEE8B4-9456-4DD7-847D-6F6C909BBB9D`, version zero; the group names the wrapper and the carrier. In the 17-member form, a tracking carrier names the joining arc as its primary identity and the two selected lines as related identities through an axis group. Each operand retains its native identity and resolved Sketch carrier.

**Need.** The boundary program and source/history join of carrier type `50AEE8B4-9456-4DD7-847D-6F6C909BBB9D`, and a stored replacement or revision join that establishes membership of each added boundary curve and the feature-time parameter intervals of its adjacent members. This join must distinguish a boundary edit from an additional unselected region.

### DR-523. Saved timeline marker position

**Question.** Which field places the saved timeline marker when the item before the first item with `s1 = 1` stores no tail state?

**Known.** `f3d.md` §3.1 "In every form, the state bytes" places the marker before the first item with `s1 = 1`. An occurrence item can follow the last item with `s1 = 0`. The scope item tails are byte-identical when the marker is directly before that occurrence item and when it is directly after it. The Browser segment is 270 bytes longer when the occurrence item is before the marker. A feature timeline can store one item run and two empty item runs, with a `Combine` scope as the final item. That `Combine` stores null current and preceding history-state identities and has a companion `RemoveBody` scope. Two earlier `Move` scopes store result states, and an `Assemble` scope directly follows the later `Move` scope. One of the two bound ASM histories terminates at the result state of the later `Move` scope. A built item after an item with `s1 = 1`, or an item with unread flags after it, makes the decoder state the timeline built through its last item or before that first item with `history.rollback-unread`. An item with unread flags immediately before that first item adds `history.rollback-bound-unread`.

**Need.** The record outside the scope item tails that changes when the marker moves across an occurrence item, bound to that occurrence item.

**Note.** Joint activation remains under DR-249.

### DR-1032. Feature item tail state bytes and the no-result flag

**Question.** What separates state byte 0 from state byte 1 of a feature item tail, what does state byte 2 record, what does a state byte above `1` record, which other states set the no-result flag `s0`, and which result-body records does a rolled-back feature that makes a body and holds no state store?

**Known.** `f3d.md` §3.1 "In every form, the state bytes" reads a feature with state byte 0 or state byte 1 set as suppressed. Every tail with state byte 0 or 1 set stores `s0 = 1`. A `Sketch` and a cut `Extrude` in one suppressed timeline group store `1` at state byte 1 and at state byte 0 respectively, each with `s1 = 0`. A directly suppressed `Combine` before the marker stores `1` at state byte 1. A suppressed `Fillet` after the marker stores `1` at state byte 1 when the `Combine` before it is active, and at state byte 0 when that `Combine` is suppressed. A `Sketch` that is the only item of its timeline can store `s0 = 1`, `s1 = 0`, state bytes `00 00 00` and null history-state identities; the saved preview draws its curves. A `Form` scope with the 196-byte tail can store `s0 = 1`, `s1 = 0`, state bytes `00 00 00` and null history-state identities in a design whose T-spline bodies have no BREP member. A rolled-back feature whose inputs changed after its computation stores `s0 = 1`, state bytes `00 00 00` and identities that name no state of the bound ASM history. The tail bytes do not separate this state from the other states that store `s0 = 1` and state bytes 0 and 1 clear. The decoder reads every feature with `s0 = 1` and state bytes 0 and 1 clear as active with no current result, this state included. It charges `feature.unevaluated-state-assumed` when state byte 2 is set, and `feature.no-result-active-assumed` when state byte 2 is clear and `s1 = 0`. The decoder reads a tail with `s0 = 1`, `s1 = 1` and state bytes 0 and 1 clear as active with no charge. That reading assumes that the states this item lists are all the states that set `s0`. The decoder reads a state byte that is not zero as set and charges `feature.state-byte-above-one` for a state byte above `1`. `f3d.md` §3.1 "This subsection gives the records of an `Extrude` item that stands after the saved rollback marker" gives the records of a rolled-back `Extrude` that was computed. `f3d.md` §3.1 "In every form, the state bytes" gives the tail of a rolled-back feature that holds no state: null history-state identities, state bytes `00 00 00`, `s0 = 1` and `s1 = 1`. The writer writes a rolled-back sketch-driven `Extrude` or `Revolve` that has no history state in that tail state, with the records of a computed item and a result body whose Scene records store the empty box, and charges `record.residual-unavailable` for each such item.

**Need.** Controlled saves of one feature suppressed directly, the same feature suppressed through a suppressed timeline group, and both together, with the state bytes of each compared. A save that sets state byte 2, with the application state that causes it. Saves of a sole `Sketch` and of a `Form` before and after a compute, with their `s0` compared. A save with the timeline marker moved before a new body-making feature that was never computed, with the result-body records of that feature compared with those of the same feature after a compute.

### DR-1005. Live flags of duplicate scope envelopes

**Question.** Same-index scope envelopes can differ only in their tail state bytes or in the suppression flags of their Thread companion tails, with no envelope bound to an ASM transition, or only in their tail state bytes and their no-result and rolled-back flags, with no envelope bound to an ASM transition and no single envelope at a primary record frame. Which envelope's tail state is live?

**Known.** `f3d.md` §3.1 "Same-index scope envelopes with the same logical payload" selects the authoritative envelope by a unique ASM transition, by equivalence of all logical payloads, or by a unique primary record frame. The decoder keeps the later envelope in stream order, also over a unique primary record frame when only the state bytes or the Thread companion suppression flags differ. Where that envelope stores flags, it keeps them. Where it stores none, it keeps the flags of the latest earlier envelope that stores them, with their byte offsets in that envelope. For each such record it charges one `feature.scope-envelope-superseded` note that names the scope, the envelope that supplied the kept flags and their values, and the byte offset and flags of each other envelope whose flags differ from the kept flags.

**Need.** A field or an order rule that selects the live envelope. The state bytes decide whether the feature is suppressed and the rolled-back flag decides where the timeline marker stands, so the selection changes the evaluated design.

### DR-520. Operation tables with an empty path, a two-reference path, or version two or three

**Question.** How does an operation table with an empty occurrence path select an occurrence, its final pose, and the saved operation state? In which frame does a version-four table with a two-reference path pose its inner occurrence, and what do the path-ordinal values select? Which pose does the final entry of a version-two or version-three direct table give?

**Known.** `f3d.md` §1.4 "**Direct operation-table poses.**" defines the direct table, whose path has one reference, for versions zero through four, and the pose of its final entry for versions zero, one, and four. Version-two and version-three direct tables have the version-one grammar. The decoder applies their final entry as the occurrence pose of a version-one table and reports `assembly.operation-table-version-assumed`. A version-four table can store path count `0`, then byte `03`, a u32 count of `0` or `1`, and that many u32 values before its references. A table with count `0` has entry cursor `FF FF FF FF` and an empty entry run. A table with count `1` has one entry; its matrix equals the matrix of one local occurrence-placement record of the same Design stream, and the values `1` and `2` occur in a Design with two component occurrences. Every reference of such a table names the root component or an operation record. Version-two and version-three tables with an empty path store the component reference directly after the path count. A version-four table with a two-reference path stores the outer occurrence-placement reference first and three u32 values after byte `02`. The decoder reads the path-ordinal value of a one-reference path, applies the table to the occurrence that the path names, and reports `assembly.operation-table-path-ordinal-assumed`. It retains every empty-path table and every two-reference table with `assembly.operation-table-pose-native`.

**Need.** The record that the u32 values after byte `02` or `03` index, and the meaning of the references between the `03` field and the entry cursor. Compare designs with one, two, and nested local occurrences, each saved before and after one position change. Vary suppression and the timeline position separately for the saved-state selector. A design with a version-two or version-three direct table, saved before and after one position change of the occurrence that the path names.

**Conflict.** A version-four table can reference the Assemble scope that precedes the final timeline item. The occurrence envelope carries a separate matrix. Their occurrence identity and pose precedence require a typed join.

**Note.** DR-113 covers pose precedence and saved-state selection across other path forms. DR-523 covers the rollback marker.

### DR-463. Analytic sketch-curve tail flag frames

**Question.** Which analytic sketch-curve tail frames carry the construction flag, and how does the type version select each frame?

**Known.** `f3d.md` §3.1 "Immediately after the 96-byte analytic payload, a planar sketch-curve record stores" describes one tail frame. A two-reference tail can start after the references with three zero bytes, byte `1`, a variable Boolean byte, and five or six further bytes before a u32 `1`. Curves reported as unprofiled have that variable byte set. The current reader's guard does not admit these frames. The same tail form occurs on the two diagonals of a rectangle, on two diagonals and a divider whose two-reference tails store the variable Boolean byte set to one, and on the four curves of a rectangle that carry the same set byte. These curves participate in the current profile arrangement.

**Need.** A type-version mapping and controlled construction/reference toggles that identify the Boolean byte and the complete tail boundary for each frame.

**Conflict.** The specified guard position differs from these stored tails.

**Note.** The decoder retains each curve and applies the admitted construction-tail rule. Endpoint incidence and geometric closure remain independent of the unresolved Boolean.

### DR-319. WorkAxis point-and-direction construction rules

**Question.** How do nonzero point parameters, `refType` values, and carrier inputs determine the authored axis construction? How do the marked references of a legacy two-point carrier relate to the scope members? Which payload follows a nonzero carrier leading-block byte, and which registered type identities select the legacy fixed carrier and support layouts?

**Known.** `f3d.md` §3.1 describes the legacy and version 1 and 4 carrier layouts. In the corroborated legacy pattern, the marked carrier references name the scope members at indices 2 and 4 in that order. The corroborated `refType` 12 pattern has zero point parameters and two inputs that resolve to point-data records in endpoint order. In both patterns, the first endpoint agrees with the stored origin and the second agrees with the origin plus the displacement. Readable stored line components project independently into a checked datum axis. Counted property and reference runs retain their readable prefixes and declared arity. Unknown input roles remain absent. Literal parameter expressions and source options remain source facts. Detached parameter owners retain their source definitions without a canonical scope binding.

**Need.** Controlled designs that vary each construction rule, nonzero point parameter, operand path, property lane and leading-block payload independently, with independently measured solved axes. Registered legacy carrier and support type identities for each fixed layout.

**Conflict.** In a two-point reading, carrier inputs can fail to resolve as points, and resolved points can disagree with the separately stored line. In either case, readable points retain unknown roles and a named assumption identifies use of the stored line. Independently eligible legacy and registered sibling carriers remain separate readings. Competition leaves the neutral construction unresolved.

**Note.** Nonzero point parameters retain a named stored-line assumption and unknown rules retain `feature.work-axis-rule-unassigned`. Their authored construction semantics remain unassigned. `feature.work-axis-leading-block-layout-assumed` retains the prior bounded one-byte leading-block reading. `feature.work-axis-unsupported-version-legacy-layout-assumed` distinguishes an unsupported modern revision from an unregistered legacy carrier. `feature.work-axis-legacy-layout-assumed` identifies fixed-layout admission independently of dynamic class spellings. Partial carriers retain actual unreadable-part records and source images; endpoint tolerance is an explicitly reported hypothesis.

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

### DR-106. Hole tap-type words above 3

**Question.** Which hole settings do Hole tap-type words other than `0`, `1`, `2`, and `3` encode?

**Known.** `f3d.md` §3.1 "A `Hole` scope starts after its indexed header with eight zero bytes" gives the settled tap-type values. A Hole can store a tap-type word that reads as ASCII text; the words `35 5F 50 6F` and `6F 6E 2F 78` occur. The other bytes of such an extent block have the layout of standard flag `0`, and the B-rep bore of the Hole has the stored `HoleDiameter`. The decoder retains any other value in the extent's `leading_word` and the `hole:extent_leading_word` source property, and reports `feature.hole-extent-leading-word-assumed` for the reading that the stored scalar geometry remains independent of it.

**Need.** The Fusion version or operation that writes a text word in the tap-type position.

**Conflict.** None.

### DR-220. Hole carrier placement forms

**Question.** Which placement does a Hole carrier state when its `refType` is not `13`, `14`, or `19`?

**Known.** `f3d.md` §3.1 "The carrier `refType` states how" defines the three admitted forms. The point-data record `refType` values `5` and `6` occur on carrier `refType` `19` and do not change its placement. A `refType` `17` record that names a reconstructed plane is the case of §3.1 "A point-data record with `refType` `17`".

**Need.** A file with a Hole carrier of any further `refType`.

**Conflict.** None.

**Note.** Such a Hole retains its native placement and reports `feature.hole-placement-unproven`.

### DR-1061. Entry point of a face-point Hole with records off its carrier

**Question.** Which point places a Hole whose carrier has `refType` `19` when its point-data records do not all lie at the carrier position and no `refType` `17` record names a reconstructed plane?

**Known.** `f3d.md` §3.1 "A point-data record with `refType` `17`" admits the reconstructed-plane form. Two further forms occur. In the first, one `refType` `17` record has a position off the carrier position, the carrier position is not on the axis through that record along the carrier direction, and the record names no reconstructed plane. The carrier stores nonzero construction parameters. The transition that drills the Hole inserts a cylinder on the axis through the record position along the carrier direction. In the second, the carrier position is zero, and one `refType` `17` record and one `refType` `5` record lie at one point. The Hole is active with no current result, so no transition drills it.

**Need.** Controlled designs that place a Hole at a face point with and without reference edges, with the carrier construction parameters varied independently, and solved axes for each. For a Hole with no current result, a design that computes it and a save that states the entry point.

**Conflict.** None.

**Note.** Such a Hole retains its native placement and reports `feature.hole-placement-point-unselected`.

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

**Known.** `f3d.md` §3.1 "A `Sketch` scope joined" gives the member order, the version gates, the identity-marked matrix, and the unmarked version-4 matrix. The independent `extraOffset` reference reaches a scalar owner linked to an `ExtraOffset` length parameter. `refType` is a u32 whose value does not change the member sequence. The scalar-role table defines the ordered `values` names and direct-reference shapes. Owner-scope references remain independent of the direct geometry slots, and the solved matrix does not select the construction rule. Code `15` has a `RotateAngle` scalar and two face-selection lineages; those operands do not distinguish the exact plane-construction subtype. `f3d.md` §3.1 "**WorkPlane scope frame.**" maps each `refType` of a `WorkPlane` placement to the scope's construction word, and "**Plane entity.**" gives the offset plane of `refType` `2` from its parent plane and `AlongDistance` value.

**Need.** We must know which references a given `refType` requires to write a placement that Fusion re-solves rather than accepts as stored. A controlled nonzero `ExtraOffset` must establish its direction and effect for each supported construction. Nonzero `isZFlip` and `axisFlipped` bytes, `bUseUDirection` values above one, and version-16 periodic-face modes require a solved-frame orientation rule.

### DR-1004. Offset WorkPlane forms outside the documented item

**Question.** What do the following encode: the `WorkPlane` scope flag byte before the zero GUID, the plane kind word of a plane entity for a `refType` other than `2`, the plane entity's flag byte after its display name, and the display corners? What selects whether a constructed plane entity stores no property block, an empty property block, or the property `EntityGenesis`? How does an offset from a planar face or a sketch profile orient its plane? Which point of the plane does a sketch on a construction plane take as the origin of its placement and of its frame record? In which order does the construction collection list its entities? Which face-parent items does the Browser segment show? What does the byte after the zero word of a construction collection with property-block presence byte `1` select, and what does a second run that differs from the first run list?

**Known.** `f3d.md` §3.1 "This subsection gives the records of one `WorkPlane`" gives the offset item with a construction-plane parent for the generation words `1234`, `8`, `36`. In that form the scope flag byte is `1` and the plane kind word is `1`. Streams with the generation words `1234`, `8`, `23` store two other forms. Both store property-block presence byte `1` and u32 `0` in the scope. The first has the `ExtraOffset` owner, flag byte `1`, and plane kind word `1`. The second has a null `ExtraOffset` reference in the scope prefix and in the placement, no `ExtraOffset` owner, flag byte `0`, an ordered reference table of three records, and plane kind word `0`. Over the other placement `refType` values, the plane kind word is `0`, or `2` for `3`, `4`, and `21`, `3` for `10`, `6` for `12`, and `8` for `13`. The display name is empty or a name, and the flag byte is `0` or `1` with either. The constructed plane entities store the property `EntityGenesis` in streams with the generation words `1234`, `8`, `36` and `1234`, `8`, `17`, presence byte `0` in streams with the generation words `1234`, `8`, `53`, and all three property-block forms in streams with the generation words `1234`, `8`, `23`. The display corners are `(a, a)` and `(b, b)` with `b = 29a`, `(-x, -y)` and `(x, y)`, or a rectangle of neither form; the origin planes store `(0.25, 0.25)` and `(2, 2)`. An offset from a planar face holds a record of type `5662F619-2281-4AD9-93DC-1FCFDA1062E7`, version `3`, in place of the placement path in its ordered reference table, and an offset from a sketch profile holds a profile selection of type `4BD53E5A-0B3E-45E5-AE8B-02044306485A`. The placement origin of a sketch on a construction plane is the plane entity's origin or another point of the plane. The sketch frame origin is the foot of the perpendicular from the design origin to the plane, or another point of the plane. The construction collection lists its entities in timeline order, except in a stream whose timeline lists one plane before its collection position. The collection's `WorkPlane` counter can be greater than the context counter. `f3d.md` §3.1 gives the byte for a collection with property-block presence byte `0`. A collection with presence byte `1` stores the byte `1`, or `0` in a stream with the generation words `1234`, `8`, `36` that stores one entity and the counters `0`, `1`, `0`. A 110-byte Browser record and a pair of the browser root's entry map show the plane entity of a face-parent item, or the item has no Browser record. Both cases occur with the generation words `1234`, `8`, `36`. In a stream with the generation words `1234`, `8`, `23`, the second run can have the count of the first run and hold other references: in a five-reference second run, four of the references occur in the first run.

**Need.** A writer that reproduces a face-parent or profile-parent offset needs the selection record and the normal direction. A reader that re-derives a sketch origin needs the origin rule. The meanings of the flag bytes, the plane kind word, and the display corners decide whether a writer can store values other than the documented ones. A writer that adds a construction entity to a collection with presence byte `1` or with two different runs needs the meaning of that byte and of the second run. A writer for another generation needs the rule that selects the property-block form of a plane entity.

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

**Known.** `f3d.md` §3.1 gives the construction-group payload and its `ordinal`, and the grouped support form of `SurfaceOffset`. The groups of one feature scope usually share one ordinal; the group ordinals of a grouped `SurfaceOffset` scope take values such as 252 and 260. DR-19 records that the ordinal's allocation event is unsettled. The decoder reads groups with different ordinals as one support-face selection and reports `feature.surface-offset-group-ordinals-assumed`.

**Need.** A save of one `SurfaceOffset` whose support faces come from two selections made at different times, with the face set that Fusion shows for it.

**Conflict.** None.

**Note.** None.

### DR-273. Scale frame whose payload length matches no form

**Question.** Which form does a legacy uniform `Scale` scope store when its payload length falls outside the documented forms?

**Known.** The legacy uniform prefix has five-reference payloads of 297, 298, and 299 bytes and six-reference payloads of 308 bytes or 310 bytes with a named tail. The payload count excludes the kind and label code units. The decoder also reads a 307- or 309-byte frame with five references or a 318-byte frame with six according to the form that has that length under the English spelling. In a 309-byte frame it checks the null GUID, reference table, and tail at the English offsets of the 299-byte payload form. It reports `feature.scale-frame-length-form-assumed`.

**Need.** A legacy uniform `Scale` scope outside the documented payload forms, with its factor and center checked against the result volume.

**Conflict.** None.

**Note.** None.

### DR-25. Base Feature six-byte fields

**Question.** What do the six-byte fields after the Base Feature body suffixes and after the Base Feature record references hold?

**Known.** `f3d.md` §3.1 "**References.**" resolves the six bytes that follow an eleven-byte element: they are the target entity ID's high half and the two reference flags. A fifteen-byte element already carries the full entity ID, so its six bytes are instead the two flags and a further u32 member of the element. The `u16 0` then `u32 1` form is that member holding `1`: the flags are both clear, and a cross-segment reference would put `1` in the second flag byte rather than in the u32. The value is uniform across a scope's body-entity run.

The class-360 primary paired with class-258 and the class-409 primary paired with class-262 use the same six-byte fields on their body, repeated passive-reference, and result-record entries as the ordinary Base Feature form. Their non-empty frame length is `262 + 52N`; their non-empty shared metadata field is two bytes, and the class-409 zero-body frame is 258 bytes with a six-byte shared metadata field. The class-444 primary paired with class-263 uses the same six-byte fields on its body, repeated passive-reference, and result-record entries; its passive count field has repeat marker `0`, its non-empty shared metadata field is two bytes, and its zero-body form has fourteen zero bytes after the shared metadata record. This fixes the class-keyed envelopes but does not assign a meaning to the six-byte fields.

The class-377 primary paired with class-259 is a separate direct body-reference envelope. Its body entity suffix is a u32 at primary-relative offset 40 with a ten-byte zero tail; it does not use the six-byte result-body element runs.

The class-290 primary paired with class-261 uses the same six-byte fields on its body, repeated passive-reference, and result-record entries. Its non-empty frame length is `261 + 52N`, its count field has six zero bytes before `u32 N`, and its shared metadata field is two bytes.

**Need.** We must know what the per-element u32 selects to write a Base Feature from a neutral model. It takes the values `1` and `0`, and it does not track the body count, so we cannot say what selects it.


### DR-259. Owner of a scope that several timeline objects name

**Question.** Which object owns a `Base Feature` scope that two or more paste or body-to-component objects name, and which fields bind an object without a later same-index header?

**Known.** `f3d.md` §3.1 "A paste or body-to-component timeline object owns" gives the owner fields; each object stores its reference to the owned scope after its counted body references. The owned scope takes its owner's timeline position. The decoder gives the scope to the object with the lowest record index whose owned runs read and reads no copy sources for it. For an unpaired object, it reads the count and references as the owned scope and leaves the copy form unread.

**Need.** A save in which two objects name one scope, with the timeline position and the body set that Fusion shows for that scope.

**Conflict.** None.

**Note.** The decoder reports `feature.timeline-owner-competing` or `feature.timeline-owner-unpaired` for the respective cases.

### DR-260. Placement of a pasted copy

**Question.** Where does a pasted copy sit relative to its source body?

**Known.** `f3d.md` §3.1 "In the scope that a paste object owns" gives the copy form. The object and scope frames store the copy bodies, their parameter-body records and the source GUID pairs. In the copy saves whose sources survive, each copy has its source's bounding box and volume. The decoder projects a paste whose sources and copies lie in one component as `MoveBody` with zero translation and no rotation. A paste whose copies lie in one component that holds none of its sources projects as `InsertBodies`, with the copies as its outputs, which their stored geometry places.

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

**Question.** Where does the section frame of a `Pipe` with a point-selection pair sit? What sets the origin and orientation of a partially consumed `Pipe` path, and does a `Pipe` with a point-selection pair whose forward and reverse fractions both equal one sweep its whole path group? How do Pipe path selections determine the neutral path-tangent and linearize controls?

**Known.** `f3d.md` defines the compact and legacy operation prefixes, the independent `isHollow` Boolean, the four canonical scalar owners, the settings record, the complete construction groups, the optional point-selection pair, the section frame and its section shape, and the outside-size and inward-thickness conventions. The two bytes after the operation are opaque. Forward and reverse path fractions remain exact native values. The point-selection pair does not establish a path origin. The decoder reads a `Pipe` with a new-body, join, or cut result operation as a circular section swept along its whole path group when its forward and reverse fractions both equal one, or one equals one and the other zero. A `Pipe` whose section frame stores the circular shape needs no further section test. Without a read shape, the decoder tests the result bodies of a new-body `Pipe` that has result bodies when no later feature names one of those bodies and every later body-changing feature names its own result bodies. The test passes when the result bodies are solids whose every face lies on a plane, on a cylinder whose radius is the outside radius or, for a hollow section, the inner radius, or on a torus whose minor radius is one of those radii, and the faces include at least one curved face of each radius. A `Pipe` that passes the test reports `feature.pipe-circular-section-inferred`. Every other `Pipe` with those fractions and an untested shape reports `feature.pipe-circular-section-assumed`; this includes a new-body `Pipe` followed by a body-changing feature that names no result bodies. `f3d.md` settles the whole-path extent of fractions that both equal one in a `Pipe` without a point-selection pair. In a `Pipe` with the pair, such fractions report `feature.pipe-whole-path-extent-assumed`. One unit and one zero fraction in a `Pipe` without a point-selection pair name a closed path. For one unit and one zero fraction, the decoder assumes that the section lies where the unit fraction starts on the path, and reports `feature.pipe-path-end-section-assumed`.

**Need.** A `Pipe` with a point-selection pair, saved with each section shape, shows where its section frame sits. Partial-path reconstruction needs the native path parameter origin and orientation. A `Pipe` with a point-selection pair and fractions of one, saved with its result body, shows which part of the path it sweeps. A `Pipe` with one unit and one zero fraction, saved with its result body, shows where the section lies on the path. Controlled tangent-chain selections and curved paths must bind the neutral path-tangent and linearize controls.

**Conflict.** None.

**Note.** Remaining Pipes project as Sweeps. An explicit circular shape and valid dimensions supply a generated circular section independently of path and extent readiness. Other section constructions report `feature.pipe-section-unresolved`. Source properties retain decoded shape labels, raw shape values, dimensions, hollow state, point selections, and parameter expressions. Available operation and path references remain independent of section readiness. Bounded forward and reverse fractions become neutral along and against fractions with `feature.pipe-path-parameterization-assumed`, except unit fractions in a `Pipe` without a point-selection pair. A generic role-`0x5` path interpretation for an Intersect or unknown operation reports `feature.pipe-path-role-assumed`. Required path-tangent and linearize controls assume false with `feature.pipe-path-tangent-assumed-false` and `feature.pipe-linearize-assumed-false`. Missing source, scalar, operation, path, target, and extent joins have separate unresolved loss codes.

### DR-61. Cross-document selection stamp and evaluation scalar

**Question.** How does the producer allocate or derive the selection-payload stamp, and what unit and evaluation event does the scalar represent?

**Known.** `f3d.md` §3.1 "A tool body-selection record with primary index `N`" defines a u64 stamp, a u32 evaluation epoch, and a finite nonnegative f64 scalar. The scalar is not an integer identity, and the epoch is not a fixed selector constant. Selectors with the same asset, context, external body, segment, property, and version can have different stamps. Different occurrences can share a stamp. A zero stamp can occur with a valid nonzero occurrence and external-body reference, so it does not establish an absent selection or a general default. The complete native selector remains the preservation unit.

**Need.** Controlled producer saves must separate selection creation, occurrence choice, selected entity, reevaluation, source version, and relink operations. A producer trace or documented evaluator contract must establish the scalar unit and event. A stamp derivation must reproduce the known cases and predict held-out saves, or identify the allocation event and persistence boundary if the value is allocated state.

### DR-62. Producer point-property mapping

**Question.** Which producer properties or creation operations select the opaque point flags and closure state, and what is the exact producer meaning of lanes 5 and 7?

**Known.** `f3d.md` §3.1 defines the versioned point tails, companion incidence, and ownership joins. Closure `(2,1)` permits a line incidence and is not an isolated-point predicate. Lane 5 requires lane 3 and positive-zero coordinates, but zero coordinates do not imply lane 5. Lane 7 identifies a point at its single incident circular curve's center. These constraints do not establish a producer API Boolean name. Neither closure nor flags select neutral construction or replace the explicit curve graph for profile admission.

**Need.** A producer trace or independently controlled saves must isolate point creation, origin creation, constraint changes, linked and reference state, visibility, and circular-center construction. The producer property mapping must predict held-out nondefault states before a writer can derive these bytes from neutral roles.

### DR-67. Base Face material side and result choice

**Question.** Which producer fields select Side 1, Side 2, Center, New Body, and New Component for a sheet-metal Base Face or Base Flange, and what do the opaque carrier suffix and repeated-u32/f64 lane mean?

**Known.** `f3d.md` §3.1 defines the stable Base Face scope, ordered source-profile pairs, persistent region identities, and separate sheet-metal settings. The one-identity carrier is valid; the enclosing scope is a different record. The repeated discriminator differs between Forward results and is not a constant Forward or feature-family code. The complete carrier suffix remains opaque and has not been shown to omit side or result choice. A Base Flange in the fixed 416-byte form with a read operation frame places the thickness along the sketch plane's positive normal in the saves examined. Family-2 profile clauses, nonconstant loop markers, and secondary loop programs also require preservation; a family-3-only primary-loop decoder does not cover every source recipe.

**Need.** Controlled saves must vary side and result choice independently while holding the sketch, regions, thickness, and rule fixed. Compare the carrier, settings, and profile programs and confirm each change against exact result topology. Resolve the ordered region identities to the corresponding sketch regions, including secondary loops.

**Note.** A Base Flange with an unread operation frame keeps its readable profile and governing thickness. Its selected region program remains native with `feature.base-flange-profile-unresolved`; its side remains native with `feature.base-flange-side-unresolved`. With a read operation frame the decoder assumes Forward and charges `feature.base-flange-side-assumed`.

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

**Note.** DerivedInstance projection keys a definition without naming-space metadata
by its stream and component record. It retains explicit body bindings to that
record and reports `feature.derived-instance-definition-unresolved`. Other legacy
GUID fallbacks retain the conflict above.

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
stores no measurement word? Which reading applies when the indexed parameter
and the fixed lane store the same distance?

**Known.** `f3d.md` §3.1 "In the current reference-aware layout, a one-sided
direction" gives the measurement word of the current reference-aware layout.
A symmetric `Extrude` without indexed parameters can store lane zero as `1.2`
cm while its operation history holds planes `12` mm on each side of its Sketch
plane, for example at `x = 138.5` mm and `x = 162.5` mm for a Sketch in the
plane `x = 150.5` mm. Its prologue stores travel direction `3`, face-extend
option `2`, and side extents `[1, 0]`.

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

### DR-293. Mesh face-group registry value

**Question.** What does the mesh face-group registry value count?

**Known.** A face-group channel lists key and GUID entries. The decoder retains field 9 and all entries, uses the triangle selectors, and reports `mesh.face-groups-assumed` when the count differs or a group has no triangles.

**Need.** A controlled face-group edit establishes the meaning of field 9.

**Conflict.** None.

**Note.** None.

### DR-115. Mesh container decode-failure taxonomy

**Question.** Which malformed or unsupported container states does `mesh.container-undecoded` cover, and does the retained error identify the failing stage?

**Known.** `f3d.md` §1.1.2 "When a document's mesh-body metadata" defines the omission: a mesh body whose metadata or referenced `.paramesh` container fails the container grammar transfers no tessellation and reports `mesh.container-undecoded` with the entry name and the decode error. B-rep evaluation then proceeds. The specification gives the container layout; the per-stage failure taxonomy awaits definition.

**Need.** Containers that fail at each stage (magic, version, reserved run, protobuf framing, name-table order, descriptor grammar, stream encoding, declared counts) must show which failures this loss covers and whether the retained error text identifies the failing stage, before a reader can distinguish malformed input from an unsupported container generation.

**Note.** A resource-limit failure returns an error through the error path.

### DR-86. Linked component-binding entity

**Question.** What is the entity that a linked component context binding names between the component reference and the context UUID?

**Known.** `f3d.md` §3.1 "A Design component naming-space type" gives the linked form. In a Design stream that uses the linked form, every component except the root component binds in this form, and several components can name the same entity. The entity can be registered to type GUID `8ACBA2E5-5A4C-4438-8351-276DE96E6982`, add-in name `Fusion`, base-type GUID `046D6232-EDAB-414D-9CB0-A2E3F115AE1C`, and type version `4`. In other streams, the components name one entity that the MetaStream registers to no type and whose record it does not index.

**Need.** A writer must emit the entity to produce the linked form. The shared entity can group components by their parent component or source document.

### DR-87. Parameter-owner flag byte

**Question.** What does the flag byte after the `variant` byte of a parameter owner store?

**Known.** `f3d.md` §3.1 "A dimension or feature-input parameter without the null owner stores `owner_record_index`" gives the byte. It is zero in almost every owner. It is one only in scalar owners whose `variant` is one, and a scalar owner can also store variant one with flag zero. The owner of a driven dimension stores variant zero and flag zero. When the same dimension is saved driving and then driven, the 104-byte owner frames of the two saves differ in one byte, the last digit of the class tag, for example `361` in the driving save and `363` in the driven save. Each tag is `256` plus the type-table index of type `D91D429C-1BB5-4640-B49F-69AF14F463E9` in its save, and every parameter owner of a save carries the tag of that save.

**Need.** The flag can mark a suppressed parameter. A design that suppresses the feature of one scalar owner, saved in each state, and a save that creates a scalar owner with `variant` one.

### DR-88. Secondary asset-folder run

**Question.** What separates the assets of the secondary asset-folder run of the top-level manifest from the assets of the first run?

**Known.** `f3d.md` §1.3 "The asset-folder tail is the unique suffix" gives both runs. The secondary run is empty in most documents. In documents that also store a toolpath, animation, or simulation asset, it lists the `Simulation Model` folder, and the first run lists the Design, toolpath, and animation folders.

**Need.** A writer must place each asset in the correct run. A document with two simulation studies, or with a simulation asset and no toolpath, shows which asset kinds the secondary run holds.

### DR-92. Current revision of a component definition

**Question.** When a component definition is stored as revisions, each binding its own context UUID, which revision is current?

**Known.** An assembly can store a component definition as two record pairs that repeat one record index. Both pairs carry the component's name and each binds its own context UUID, and neither UUID occurs elsewhere in the design. The later pair is longer and carries typed references.

**Need.** A design whose persistent identities use one of the two UUIDs, or whose component properties differ between the revisions, must show which revision Fusion treats as current.

**Conflict.** Treating either revision as the only binding discards a naming space a persistent identity could select.

### DR-93. Sheet-metal rule word and entry run

**Question.** What does the u32 word at rule-record offset 21 state, how long is the rule's entry run, and which entries set bend radius, neutral-axis factor and relief?

**Known.** A sheet-metal rule record can store the word 3. It is followed by four LP-UTF16 expression and f64 value entries (`Thickness`, `1`, `Thickness * 0.5`, `Thickness * 2.0`), then a zero u32 and the `Thickness` entry again.

**Need.** Controlled edits of thickness, bend radius, neutral-axis factor and relief, with the resulting geometry measured, must identify each entry and the entry-run boundary. Rules with different entry counts must identify the rule word.

**Conflict.** Treating the word as an entry count reads one entry fewer than the record holds.

**Note.** The decoder retains the rule controls in the source record and reports `feature.sheet-metal-rule-controls-native`.

### DR-94. Freedom entry word and trailing value

**Question.** What does the u32 word before a freedom code state, and what does the f64 after the code of a word-`1` entry state?

**Known.** Word-`3` entries occur in current-document joints. Word-`1` entries occur in the motion records of `As-built` joints in inserted designs. The trailing f64 of those entries is `0` or 2π. Their freedom codes select the same motion as word-`3` codes: each word-`1` code-`2` joint's two connector frames share an origin and a z axis.

**Need.** Joints that differ only in their motion settings, saved once with each word, would fix what the word selects. Varying such a joint's angle would show whether the trailing value tracks it.

**Conflict.** Treating the trailing value as an angle, a period, or a limit states a meaning that the stored values do not fix.

### DR-95. Rectangular-pattern direction selection kind `16` and the reference at offset 138

**Question.** What does selection kind `16` select in a rectangular-pattern direction record, what do its origin and displacement mean, and which objects do the auxiliary selection references name?

**Known.** `f3d.md` §3.1 "A rectangular-pattern direction record has a 220-byte, 221-byte, 195-byte, 196-byte, 215-byte, or 216-byte form" gives the frame, the U-then-V order, and the direction as the normalized displacement. Kind `8` selects a linear edge, with the edge start point and the edge vector. A kind-`16` record stores a unit displacement and an origin on the selected entity. The reference at offset 138 names a record outside the scope reference table. The 215-byte extent form and the 216-byte spacing form have two further marked references after its selection reference; these links remain native.

**Need.** A writer must emit the selection kind, the origin, and the reference at offset 138 for a direction that does not come from a linear edge, and a reader must know whether a kind-`16` origin is a point of a planar face, an axis, or another entity. The linked forms also need the two further selection links bound to their source objects.

**Note.** A kind-`16` direction record can store displacement `(0, 0, 1)` and U distance `-1.5` cm while a payload record supplies V displacement `(1, 0, 0)`, with both counts two and V distance `1.5` cm. The decoder transfers the two directions and their 15 mm spacings. The linked forms retain their two further selection links with `feature.rectangular-pattern-direction-links-native`.


### DR-654. Stored profile loop curves off the selected material boundary

**Question.** Which sketch curves does a typed profile loop list when two coincident curves share one boundary interval, which piece of a line that ends inside the selected material does it list, and in which of the regions that the line enters does a loop list it?

**Known.** `f3d.md` "When the loops of a typed or compact profile group join the Sketch exactly" gives the transition reading. Each stored entry has a decoded piece on a selected atomic boundary, a line or circle with a shared boundary interval under `feature.profile-coincident-boundary-assumed`, or a piece in the interior of a selected region, which the loop traverses as a slit under `feature.profile-partial-entry-assumed`. For such a line, the second and third entry fields can equal the ordinal and count of the line piece inside the region when the other curves of that region divide the line. Two coincident lines listed as separate entries resolve under the transition reading: the alias line takes its certified boundary interval and charges `feature.profile-coincident-boundary-assumed`, and an entry with several pieces, or a piece with no identity use in the selected boundary, also charges `feature.profile-partial-entry-assumed`. A Cut whose stored transition inserts, deletes, and updates no face has no changed plane and no certificate, so its selection stays native.

**Need.** Controlled Extrudes of one region with a line that ends inside it, at several positions and across several regions, with the stored loops and the resulting end caps, to state which piece a loop lists for such a line and when a region lists it.

**Conflict.** None.

### DR-97. Freedom entry second value `0`

**Question.** What does a freedom entry state when its second f64 is `0` rather than π?

**Known.** `f3d.md` "Assembly operands" gives the freedom entry and its second f64. A zero second value occurs in a word-`1` rotation entry whose joint record enables both limit flags, with a −58° minimum and a 0° maximum. Word-`1` rotation entries of unlimited joints and word-`1` translation entries of limited joints store π.

**Need.** A reader must know whether the value bounds the freedom, so that it can tell whether a limited rotation's range is complete without it. Rotations saved with and without limits, and with ranges of different widths, would fix what it states.

**Conflict.** Treating the value as a period or as a range bound states a meaning that one stored entry does not fix.

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

### DR-891. Fixed Chamfer face groups

**Question.** Does the fixed specification of a Chamfer scope apply to each role-`0x0000001000000000` face group beside the scope's one role-`0x0000000800000000` edge group, and in a two-distance scope, which distance applies to each side of a chamfer face that the transition inserts along a face-group edge?

**Known.** Groups and the fixed scalars store no assignment cross-reference. The decoder applies the fixed specification of the edge-role group to each face-role group of the scope, and charges `feature.chamfer-fixed-face-group-assumed` for this assumption. A fixed two-distance Chamfer with distances 0.5 mm and 0.5 mm, one edge-role group, and one face-role group puts every chamfer face along a face-group edge 0.500 mm from that edge, which is the stored setback. Equal distances give the same face for both orientations.

**Need.** A fixed two-distance Chamfer with unequal distances, one edge-role group, and one face-role group shows which distance applies to each side of a face-group chamfer face.

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

**Note.** The decoder retains each profile operand in the native scope. When all operands of an Extrude group name the same planar Sketch and each carries a region selection, the decoder composes them as the union of their selected regions under `feature.extrude-profile-group-union-assumed`. Every other multi-operand group, and any same-sketch group whose composition cannot run (including on a spatial Sketch), stays native and emits `feature.extrude-profile-group-native`.

### DR-257. Entity-selection serialization form

**Question.** What does the u32 serialization form after `u32 2` in a nested entity-selection member encode?

**Known.** `f3d.md` §3.1 "A nested entity-selection member has primary indexed record `N`." states that the field is `0` or `1`. The field layout after it is the same for both values. Every member in one Design stream stores the same value. Value `1` occurs in documents whose product version is an ASM 231 or 232 Windows build; documents from the same builds also store value `0`. Members of both values resolve their identities to Sketch curves that close Extrude profile regions and to Revolve axis lines.

**Need.** A pair of documents that differ only in the form value, and the Fusion setting or document generation that produces each value.

### DR-258. Second marked reference of the 87-byte scope tail

**Question.** What record does the marked reference at tail byte 30 of an 87-byte parameter-scope tail name, and does it change the operation of its scope?

**Known.** `f3d.md` §3.1 "A parameter scope is one logical indexed record" states that tail byte 30 of the 87-byte tail is `0x01` when it begins a second marked reference, and that the preceding history-state identity is then at tail offset 41. In `SplitFace` scopes the reference names a record index that no indexed header in the stream carries. These `SplitFace` scopes have the same splitting-tool and target groups as the 77- and 78-byte forms, and the projection does not read the reference.

**Need.** Controlled saves of one `SplitFace` before and after an edit that creates the reference, with the record that carries the referenced index.

### DR-263. Sketch Geometry member byte 34

**Question.** What sketch state does byte 34 of the sketch container's Geometry member encode?

**Known.** `f3d.md` §3.1 "The current sketch entity container has type GUID" gives the member layout. Byte 34 is `0` or `1` and lies between the u32 ordinal at member offset 30 and the visibility flag at offset 35. Its value is independent of the visibility flag, the ordinal collection, the placement orientation, and the planar or spatial form of the sketch geometry. Member type version 3 stores the same `0`/`1` byte between its ordinal and its visibility flag.

**Need.** Saves of one sketch that change one per-sketch setting at a time (Slice, 3D Sketch, Show Profile, Show Points, Show Dimensions, Show Constraints, Show Projected Geometries), each recording the value of byte 34.

**Note.** The decoder stores byte 34 in the native sketch visibility field `sketch_flag` and emits `sketch.geometry-flag-unprojected` when it is `1`.

### DR-264. Sketch geometry group fields

**Question.** What do byte `01` at offset 49 and the f32 values at offsets 62 and 66 of a sketch geometry group store, and which operations write a group?

**Known.** `f3d.md` §3.1 "A sketch geometry group has type GUID" gives the layout and the ownership joins. Every group stores `01` at offset 49 and f32 `1.0` at offsets 62 and 66. The sketch that owns a group is named by the reference table of a `MeshSection` scope. Fusion 2706.1.17 writes one group for `Create Mesh Section Sketch` on a mesh body in a `Base Feature` with the XY construction plane. The group has the `86 + 11N` form, `01` at offset 49, f32 `1.0` at offsets 62 and 66, ordinal `1`, and 25 member references.

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

**Question.** What does the u32 before the marked body-group reference of a `Reverse Normal` scope encode when it is not `1`?

**Known.** `f3d.md` §3.1 "A `Reverse Normal` scope reverses" gives u32 `1` at offset 21 of the compact prefix and offset 25 of the shifted prefix, for any number of body-group members and for the linked scopes of a Reverse Normal of several components. One marked reference to the body group follows it. The decoder keeps a scope with another value as a native feature.

**Need.** A `Reverse Normal` scope with a value other than `1`, and the selected bodies as the application shows them.

**Conflict.** None.

**Note.** None.

### DR-303. Link GUID shared by several Reverse Normal timeline items

**Question.** Can one non-null link GUID occur in the linked scopes of more than one `Reverse Normal` timeline item, and how does a reader then assign the linked scopes that are not timeline items?

**Known.** `f3d.md` §3.1 "A `Reverse Normal` scope reverses" gives one shared non-null link GUID for the linked scopes of one item, with the scopes that the timeline does not name taking the positions directly after the named scope.

**Need.** A save with two Reverse Normal items of several components each, and the link GUIDs of their linked scopes.

**Conflict.** None.

**Note.** When several timeline items carry one link GUID, the decoder leaves the linked scopes that are not timeline items without a timeline position and reports `feature.reverse-normal-link-shared` for each. A linked scope whose link GUID no timeline item carries takes no position and is reported as `feature.reverse-normal-link-unplaced`.

### DR-894. Order of the linked Reverse Normal scopes after their timeline item

**Question.** In what order do the linked scopes of a `Reverse Normal` timeline item that the timeline does not name follow the named scope?

**Known.** `f3d.md` §3.1 "A `Reverse Normal` scope reverses" gives the timeline positions directly after the named scope to the other linked scopes of the item. Each linked scope selects the bodies of its own component.

**Need.** A save with a Reverse Normal of three or more components, and the order of the linked scopes that Fusion applies or shows.

**Conflict.** None.

**Note.** The decoder places these scopes in record order and reports `feature.reverse-normal-link-order-assumed` for each scope when an item has two or more of them.

### DR-308. Canvas flag byte before the second boundary

**Question.** What does the flag byte before the second boundary segment of a Canvas geometry record select, and does a zero value change the image placement or display?

**Known.** `f3d.md` §3.1 "Canvas geometry has eight fixed prefix layouts." gives the byte position in each layout. The byte is zero in the legacy layout and zero or one in the other layouts. A modern Canvas with a zero byte stores both boundary segments, the plane reference, and the payload frame at the same offsets as a modern Canvas with a one byte.

**Need.** Controlled saves of one Canvas that toggle each Canvas display and selection option, compared byte for byte, to find the option that sets this byte.

**Conflict.** None.

**Note.** The decoder places a Canvas with a zero byte in a non-legacy layout by its boundary segments and payload frame, keeps the byte in the native `second_boundary_flag` field, and reports `appearance.reference-image-flag-assumed`.

### DR-881. Tracked-entry record variants

**Question.** What selects the 37-byte or the 33-byte form of a side-face entry and the token-string form of a body entry (type `335CC7BD-BBE5-40AA-9E8C-5EC99D427A3A`), what does the reference that other body entries (type `ABA79BE7-D8D2-4C17-B59B-C2035E111F77`) store in place of the null reference name, and what do the two operands after the sense word of a curve-record entry (type `2DEE34BE-FA70-4D54-979C-164D86FDA786`) encode?

**Known.** `f3d.md` §3.1 "**Tracked entries of an `Extrude`.**" gives the three records and the key of a side-face entry. Streams with serializer magic `1234` store both side-face forms. A body entry can also store each side token as its own LP-ASCII string, with no offset list, and other body-entry prefixes occur. One curve record of a side-face entry stores one counted curve list in place of two. The GUID of a body entry is the browser-node GUID of the result body. The side-face entries of an `Extrude` in a design whose timeline lists `Sketch` items and one `Extrude` are in the 37-byte form, and `f3d.md` §3.1 "**Tracked results of a circle profile.**" gives the curve record of a circle profile. In other designs, the tracked-results table of one `Extrude` holds side-face entries of the 37-byte form, of the 33-byte form, or of both forms. Entries with the same sense word in their curve entry and the same scope byte at offset 27 occur in both forms.

**Need.** A writer must choose the side-face form and fill the body entry's reference and the curve entries' operands.

### DR-882. Timeline-context counters and context value

**Question.** What do the `Dc<Family>MetaType` counters and the u64 context value of the timeline context record count?

**Known.** `f3d.md` §3.1 "**Timeline context.**" gives the record layout. A design with one `Sketch` and one `Extrude` stores the counters `1` and `1` and the context value `2`. In a stream with serializer magic `1234`, each counter is at least the largest family ordinal of the timeline items of its family, and it can exceed it. Counters can name families that have no item in the timeline. The context value can equal the number of timeline items, be `0`, or exceed both the number of items and the sum of the counters. A design whose timeline holds one `Base Feature` item stores the counter `DcBaseFeatureMetaType = 1` and the context value `0`. `f3d.md` §3.1 "**Design records of Sketch items.**" gives the counter of `Sketch` items and the context value of a design whose timeline holds only `Sketch` items, or only `Sketch` and `Extrude` items, and whose parameters keep their `d<n>` names. In a design whose timeline also holds items of other families, `DcSketchMetaType` can exceed the number of `Sketch` items, and the timeline context can omit it. A design whose parameters were renamed can store a context value below the largest `n` of its remaining `d<n>` names. A design whose timeline lists `Sketch`, `Extrude`, and `Sketch` stores the context value `2` or `3`; `Sketch`, `Extrude`, `Sketch`, `Sketch` stores `4`; and `Sketch`, `Extrude`, `Sketch`, `Extrude` stores `4`. In each, `DcSketchMetaType` and `DcExtrudeFeatureMetaType` are the numbers of items of their families. The timeline context of a design whose timeline lists `Sketch` items and one `Extrude` references one browser-node member, the result body's.

**Need.** A writer must write correct values for designs with other item sets. The counters against the ordinals of deleted items, and the context value against the rolled-back position and the deleted items, would settle it. A design whose timeline lists two or more `Sketch` items before its first `Extrude` would give the value for that case.

### DR-883. Extrude side-face walk outside one-sided loops of lines

**Question.** Which curve takes the lowest side token of a loop that holds an arc? In which order do the regions of a profile of several regions take their side-token ranges? Which curve starts the walk, and in which direction, for a reversed-direction, two-sided, or symmetric `Extrude`, and a stream whose serializer magic is not `1234`? In which order does the identity record store the hole loops of a region and the entries of a loop? For a loop that is a hole of one region and the exterior loop of another, which side faces are tracked, and which sense do their curve records store? Which sense does an exterior loop's curve record store in a profile of several regions?

**Known.** `f3d.md` §3.1 "**Side-face walk.**" gives the ring of each loop of lines and arcs of a one-sided join, new-body, or cut `Extrude` with reversed-direction byte `0` in a stream with serializer magic `1234`, its orientation, the contiguous token range of the loop and its direction, and "**Walk of a loop of lines.**" gives the curve that takes the lowest token of an exterior or hole loop of lines. In streams with serializer magic `1234`, the loops that hold arcs belong to two-sided, symmetric, or reversed-direction `Extrude` features or to profiles of several regions, and the only such loops of a one-sided `Extrude` with reversed-direction byte `0` are hole loops of arcs only. The range and the direction that "**Side-face walk.**" gives for an exterior loop that holds an arc are those of the exterior loops of lines and of the loops with arcs in streams with other serializer magic. In streams with other serializer magic, the lowest token of a loop that holds an arc goes to the curve that the walk of a loop of lines names when `C` is the loop curve of the largest record index on some loops and to another curve on others; the largest or smallest record index or primary identity of the loop's curves, of its lines, of its arcs, or of its sketch points each give the start on some loops only. In streams with other serializer magic, the walk of a loop of lines names the side faces of most loops of lines. On the others, the tracked side-face keys run in the walk's direction from a start one line away, and the stored body has no side face for one loop line or for any loop line. With reversed-direction byte `1`, the side tokens of a loop of lines run against the ring. A new-body `Extrude` whose profile was selected again can store the side tokens of its one loop of lines from `7`; its tracked-results table then keeps side entries with the keys `3` to `6` whose curves are no longer in the loop. The side face of a whole-curve loop entry can be split into two tracked side entries, whose curve records store the operands `1` and `3`, and `3` and `3`. The entries of a loop stand in ring or reverse ring order in some loops; ascending or descending primary identity, record index, or side token each give the stored order of few loops. `f3d.md` §3.1 "**Profile with circular holes.**" gives the records and side tokens of a region whose hole loops are circles, and the ranges of a region with one hole loop. In streams with serializer magic `1234`, the exterior loop is the first loop of every region whose hole loops are circles; in regions with hole loops of other kinds and in streams with other serializer magic, it is the first loop of most regions and stands at another position in some. Its side faces take the tokens from `3` at every position in each region with hole loops of streams with serializer magic `1234`, and in most such regions of other streams. In a region with two or more hole loops, the exterior loop takes the range from `3`, and the hole ranges follow the stored order of the hole loops in some regions and another order in others. In such regions whose cap faces hold every hole loop, all in streams with other serializer magic, the order of the hole loops on the cap face is the order of their ranges. The stored order of two or more one-entry hole loops is ascending in their primary identities in some regions, descending in others, and neither ascending nor descending in others; in streams with serializer magic `1234` it is neither in most regions. The record order of their sketch curves, the coordinates of the circle centres, the radii, and the distance and the angle of the centres about an exterior circle's centre each give the stored order in some regions only. The side face of a one-sided or two-sided `Extrude` whose profile is one loop of one entry has side token `3`. A symmetric `Extrude` of such a profile can carry side token `5` without side token `3`. A loop whose curves form a hole of one region and the exterior loop of another region of the same profile has no single stored form: a one-circle loop of that kind can have no tracked side entry, a side entry whose curve record stores sense `1`, or one that stores all ones, and a loop of several whole-curve entries can have side entries with sense `1` or none.

**Need.** A one-sided reversal-`0` `Extrude` of a stream with serializer magic `1234` whose profile loop holds an arc: it gives the curve that takes the lowest token of such a loop and shows the range and its direction for an exterior loop that holds an arc. The stored order of the hole loops of a region with two or more holes and the order of their side-token ranges, which a writer of such a region must choose. The region order of multi-region profiles against the stored region order and the region geometry. The `AlongDistance` values of reversed-direction `Extrude` features against their token direction. A two-sided or symmetric `Extrude` of a loop of several lines. An `Extrude` whose stored profile normal is opposite its Sketch's normal, to tell whether the `AlongDistance` sign or the extrusion direction against the Sketch normal orders the tokens. The records of a split side face. A writer names side faces by the walk and stores the entries of a loop in ring order.

### DR-1040. Side entries and side tokens of a sketch-driven Revolve

**Question.** Which profile loop entries of a sketch-driven `Revolve` take side entries in its tracked-results table, in which order do their side tokens follow the loop, and when does the table hold no body entry or more than one?

**Known.** `f3d.md` §3.1 "**Revolve tracked results.**" gives the table: consecutive keys from `3`, side entries of type `335CC7BD-BBE5-40AA-9E8C-5EC99D427A3A` and body entries of type `ABA79BE7-D8D2-4C17-B59B-C2035E111F77`, and the key after the last key. For a profile of one region with one loop, a table holds one of four runs: side entries from key `3` and then one body entry; side entries only; two runs of side entries that each end with a body entry; or side entries, one body entry, and a second run of side entries that ends the table. In a table of the first form the side entries can name the loop curves other than the axis line, one entry for each curve. In other tables a side entry names a curve that the stored loop does not hold, or a loop curve other than the axis line has no side entry. The side tokens of consecutive keys name curves that meet at a common end in some loops and curves that do not meet in others. The faces of a partial turn can carry the cap tokens `1` and `2` beside the side tokens. DR-883 gives the corresponding questions for the `Extrude`.

**Need.** A one-sided full-turn new-body `Revolve` of a one-region profile of lines and arcs, saved in a stream with serializer magic `1234`, once with a profile line as the axis and once with a construction line beside the profile as the axis: the side entries and the side tokens of each, compared with the ring of "**Side-face walk.**". The same `Revolve` saved again after its profile is selected again following a sketch edit, with its table compared.

### DR-1041. Prologue forms of the Revolve axis curve identity, carrier links, and side entry

**Question.** What selects the 49-byte form of a `Revolve` axis curve identity over the 45-byte form, the 37-byte and the 157-byte forms of a carrier link over the 33-byte form, and the 33-byte form of a `Revolve` side entry over the 37-byte form?

**Known.** `f3d.md` §3.1 "**Axis curve identity.**" gives the two forms of record `A + 3`: after the indexed header, ten zero bytes, or nine zero bytes, the byte `1`, and u32 `0`; then eight zero bytes and the two u64 identities. In the 49-byte form, the byte `1` and u32 `0` stand where the 157-byte carrier link stores its property-block presence byte and its property count. Record `C + 2` of the axis tracking carrier stores the same identities in the 45-byte form, also when record `A + 3` has the 49-byte form. "**Axis tracking carrier.**" gives the three forms of a carrier link of type `CB4F4CC2-9D6F-48BA-9B1F-20C1992C9C9C`: 33 bytes, with ten zero bytes before the reference to the next record; 37 bytes, with nine zero bytes, the byte `1`, and u32 `0` before it; and the 157-byte record `C + 1` with a property block. In a stream with serializer magic `1234`, record `C + 1` of an axis tracking carrier is a 33-byte, 37-byte, or 157-byte link, and records `C + 3` and `C + 5` are 33-byte links or 37-byte links; 37-byte records `C + 3` and `C + 5` occur with a 157-byte record `C + 1`. "**Revolve tracked results.**" gives the 37-byte side entry, whose reference follows nine zero bytes, the byte `1`, and u32 `0`, and the 33-byte side entry, whose reference follows ten zero bytes. One tracked-results table can hold side entries of both forms.

**Need.** `Revolve` features with a sketch-line axis saved in a stream with serializer magic `1234`, before and after an edit of the axis selection and of the profile, with the form of each record compared.

### DR-1042. The 76-byte item tail of a Revolve

**Question.** What selects the 76-byte item tail of a `Revolve` over the 77-byte item tail in a plain-reference stream?

**Known.** `f3d.md` §3.1 "**Revolve item tail.**" gives the forms. The 76-byte tail stores the family ordinal at tail offset 0, the custom-label count at 8, the state bytes at 16, the tracked-results table at 19, the preceding history-state identity at 30, the evaluation epoch at 34, the evaluation duration at 38, the second evaluation epoch at 46, the item-tail companion at 50, the no-result flag at 61, the rolled-back flag at 62, the record-timestamp table at 63, and two zero bytes at 74. A plain-reference stream stores the 76-byte tail for its `Sketch`, `Extrude`, and `Revolve` items when its MetaStream header stores a serializer magic from `134223405` to `134223991`, and the 77-byte or the 87-byte tail when it stores serializer magic `134224396` or a larger value.

**Need.** One design of a `Sketch`, an `Extrude`, and a `Revolve`, saved by builds that write the serializer magic values between `134223991` and `134224396`: the item tail of each scope compared.

### DR-1043. Scope frame of a one-sided Revolve with serializer magic 1234

**Question.** What is the scope frame of a one-sided version-2 sketch-driven `Revolve` in a stream with serializer magic `1234`?

**Known.** `f3d.md` §3.1 "**Revolve scope frame.**" gives the one-sided version-2 frame of a plain-reference stream ([`sketch_revolve_scope_frame`](../layouts/f3d.md#sketch_revolve_scope_frame)), which the 77-byte item tail follows. In a stream with serializer magic `1234`, a two-sided version-2 `Revolve` scope stores the fields of that frame with these differences: the byte `0` at offset 35; the Two Sides flag `1` at offset 37; a marked reference to the `AgainstAngle` owner in the opposite-angle slot at offset 38, so that each later field stands ten bytes later; the zero u32 at offset 99; the ordered-reference count `7` at offset 205, with the ordered references in ascending record index; and the 78-byte item tail.

**Need.** A one-sided full-turn new-body `Revolve` of a Sketch profile about a line of the same Sketch, saved in a stream with serializer magic `1234`: its scope frame compared field by field with `sketch_revolve_scope_frame`.

### DR-1044. Body-presentation owner and Body owner of a Revolve result body

**Question.** What is the layout of the body-presentation owner of a sketch-driven `Revolve`'s result body, which form of the `Body` owner does that result body store, and what selects it?

**Known.** `f3d.md` §3.1 "**Revolve result body.**" gives the browser-node member and the body-presentation owner, which references one `Body` owner of type `CD57BC48-50EC-47DC-975A-FB6DEA72F4DA`, type version `4`, twice. That `Body` owner has no paired header. After the indexed header it stores nine zero bytes, the byte `1`, and u32 `0`, and it references the snapshot body-map companion (type `E55D0457-898F-4A8E-B1A5-C85BDBA8794F`), the body map (type `74CA4562-59F8-4C97-8AD1-F8297C21F9AA`), a record of type `372234A0-2A6C-4122-8047-E74DBA04F326`, and the component aggregate. Its length is 89, 107, 151, or 1163 bytes in a plain-reference stream and 415 bytes in a stream with serializer magic `1234`; the 107-, 151-, 1163- and 415-byte forms also reference body-presentation owners. "Records of a sketch-driven Extrude" gives the 103-byte form of the `Body` owner of an `Extrude`. The body-presentation owner of type version `18` stores, after the indexed header, ten zero bytes, or nine zero bytes, the byte `1`, and u32 `0`, and then u32 `2`. The body-presentation owner of type version `19` starts with the prefix `sketch_extrude_body_presentation_owner_prefix` and the LP-UTF16 name `0_` followed by its own decimal record index.

**Need.** A design of one Sketch and one one-sided new-body `Revolve`, saved in a stream with serializer magic `1234`: the `Body` owner of its result body compared with the 103-byte form.

### DR-1045. Clauses and body recipe of a body that other features changed

**Question.** When the clause identities of the whole-body member of a join or cut name scopes of families other than `Extrude`, which clauses, forms, and recipe entries does the member store, and what do the further items of its body recipe hold?

**Known.** `f3d.md` §3.1 "**Whole-body member.**" and "**Body recipe.**" give the whole-body member and the body recipe, and "**A body that `Extrude` items made.**" gives the clauses and entries of a body that only `Extrude` items made. The clause forms include `2`, `3`, `4`, `8`, `9`, `32`, `33`, `64`, `65`, `128`, and `129`, and a whole-body member that a `Split` owns can store one clause of form `7` that names a new-body `Extrude` scope. By form, a clause identity can name a scope of these families: `2` a `Loft`, `Mirror`, or `SurfaceOffset` scope; `3` a `Base Feature` scope; `4` an `Extrude` or `Sweep` scope; `8` and `9` a cut `Extrude` or a `Combine` scope; `32` a new-body `Extrude`, `Pipe`, `SurfaceStitch`, or `Thicken` scope; `33` an `Extrude` scope; `64` and `65` a `Split` scope; `128` a `Mirror` scope; `129` a `Mirror` or `Loft` scope. A clause identity of form `4`, `33`, or `65` can name no scope of the persistent-id table. The entries of the recipe can name an identity that no clause of the member stores, and they can differ from the body groups of each body of the preceding state. A recipe whose member stores one clause of form `3` can store the further-item count `1` and one 24-byte item (u32 `1`, u64 `0`, u32 `1`, a u32 persistent identity of the Design stream, u32 `0`), then u32 `16`, the family name, and eight zero bytes.

**Need.** A body made by a new-body `Extrude`, then split by a `Split`, mirrored by a `Mirror`, or combined by a `Combine`, then selected by a join `Extrude`: the clauses and the recipe entries of the join's whole-body member compared with the body groups of the selected body in the preceding state.

### DR-1046. Body entries of a cut, and of a join with several bodies

**Question.** Which cuts store body entries, and what do the body entries and their body recipes hold for a cut and for a join whose body-operand group has several members or whose tracked-results table holds several body entries?

**Known.** `f3d.md` §3.1 "**Tracked results of a join or cut.**" gives the body entry of a join and the two forms of the table of a cut. The table of a cut whose result-body list is empty can hold one body entry in the form of the body entry of a join. The table of a cut whose result-body list names browser-node members can hold several body entries whose run counts are greater than `1`, some of them with a reference in the slot at offset 21. The table of a join can hold several body entries, each in the form of the body entry of a join.

**Need.** A cut that splits one body into two bodies, a cut that removes material from one body and leaves one body, and a join of two bodies: their tracked-results tables, body entries, and body recipes compared.

### DR-1047. Groups of the faces that a join or cut inserts

**Question.** Which groups does a face carry when the transition of a join or cut inserts its `generic_tag_attrib_def` attribute?

**Known.** `f3d.md` §3.1 "**Faces that a join or cut changes.**" gives the faces whose attribute the transition updates. An attribute that the transition inserts holds face groups. Each group names identities of the item only, or identities of earlier items only, and a group names one or more identities.

**Need.** A cut through one face of a box `Extrude`, and a join whose start cap lies on a face of the box: the groups of each face in the preceding state and in the current state compared, with each inserted face paired with the faces that the transition deletes.

### DR-1048. Dependency entries of a join or cut after other items, and selection identities

**Question.** When items that store a current identity stand between the creating `Extrude` and a join or cut, which records do the scope (flag `0x400`) and the whole-body member read, and with which flags? How are the local identities of the persistent selection identities numbered across the items of a timeline?

**Known.** `f3d.md` §3.1 "**Owner collection, `B65B5682` record, and dependency table.**" gives the input entries of a join or cut with no such item between. The scope reads records of earlier timeline items with flag `0x400`, and each of them is a record that the whole-body member or the profile selection reads. The whole-body member reads records with the flags `1`, `2`, and `0x10`, and a record that it reads with flag `2` belongs to an earlier timeline item. The start record and the profile selection of one item each name a persistent selection identity (type `C46D3EEB-40E1-43FA-83DC-9CB204335417`) with a u64 local identity at offset 21; the two local identities of one item differ by one or by more than one.

**Need.** A new-body `Extrude`, a `Fillet` of one of its edges, and then a cut `Extrude` of the body: the dependency entries of the cut scope and of its whole-body member, and the local identities of the selection identities of both `Extrude` items, compared with the same design without the `Fillet`.

### DR-1050. Record-timestamp tables of the body-operand group and the whole-body member

**Question.** Which records do record `G + 1` of the body-operand group and record `M + 1` of a whole-body member list, which record does each name as its owner, and in which form does record `M + 1` store them?

**Known.** `f3d.md` §3.1 "**Body-operand group.**" gives record `G + 1` as the record-timestamp table of the group, and "**Whole-body member.**" gives record `M + 1`, which the member references as its record-timestamp table, as a record of type `6CCF41D5-40BE-48ED-A834-18F3EAED6C57`. `f3d.md` §3.1 gives the record-timestamp table: the prefix with `01 01` and a u32 entry count, entries of `01 + u32 record_index + six zero bytes + u64 timestamp`, an owner entry in the same form, and eight zero bytes. DR-916 gives the records that the tables of the records of a new-body `Extrude` list in a design of `Sketch` items and one `Extrude`.

**Need.** A Sketch, a new-body `Extrude`, and a join `Extrude` of its body, saved, then a second join `Extrude` of the same body added and saved: the entries and the owner entry of records `G + 1` and `M + 1` of each join compared with the records of the scopes that the clauses of its whole-body member name.

### DR-1051. Context GUID of the whole-body member

**Question.** Which GUID does the whole-body member of a join or cut store as its context GUID?

**Known.** `f3d.md` §3.1 "**Whole-body member.**" gives the LP-UTF16 asset GUID and the LP-UTF16 context GUID of the member, each of 36 code units; in `whole_body_member_single_clause` their length words stand at offsets 52 and 128. The asset GUID is the asset GUID of the profile selection. The context GUID of a persistent selection identity of the start record or of the profile selection is the third LP-UTF16 GUID of the component aggregate (type `E03784ED-5E19-4E14-B9F2-3B07017018CD`) ("**Persistent selection identities.**"). The decoder frames the member and keeps neither GUID.

**Need.** A Sketch, a new-body `Extrude`, and a join `Extrude` of its body in the root component, and the same three features in a child component: the context GUID of the join's whole-body member compared with the GUIDs of the component aggregate and with the context GUID of the join's persistent selection identities.

### DR-1052. GUID and tokens of the body entry of a join

**Question.** Which GUID does the body entry of a join store, and which side keys of the join's tracked-results table do its tokens list?

**Known.** `f3d.md` §3.1 "**Tracked results of a join or cut.**" gives the body entry of a join: the LP-UTF16 GUID of 36 code units at offset 22, u32 `0` at offset 98, the run count `1` at offset 102, the tokens, and the u64 first persistent identity of the join scope. The GUID of the body entry of a new-body `Extrude` is the browser-node GUID of its result body (DR-881), and its tokens are the side tokens `3` to `k + 2` ("**Tracked-results table.**"). In the fixed frame of a join, the result-body node count is `0` ("**Join or cut scope frame.**").

**Need.** A Sketch, a new-body `Extrude` of a box, and a join `Extrude` whose profile lies on a face of the box, saved once with every side face of the join outside the box and once with some of its side faces inside the box: the GUID of the join's body entry compared with the GUIDs of the browser nodes and of the body entry of the creating `Extrude`, and the tokens compared with the keys of the side entries of the join's table.

### DR-1054. Bulletin board of a state that changes an earlier body

**Question.** Which entity slots does the bulletin board of a history state keep as updates, and which does it delete and insert, when the item of the state changes a body of the state before it?

**Known.** `f3d.md` §3.1 "**History chain.**" gives the `delta_state` chain of the `.smbh` history: one bulletin board for each state after the root state, with owner reference equal to the node index of the state and number `2`. The board of a state whose item makes a new body holds only insertions. The board of a state whose item changes a body of the state before it, such as a join or cut `Extrude`, a fillet, a chamfer or a pattern, holds insertions, deletions and updates (`asm.md` §3.2). The rule that selects, for each record of the changed body, an update of its slot or a deletion of its slot with an insertion of a new slot is open.

**Need.** A Sketch, a new-body `Extrude` of a box, and a join `Extrude` whose profile lies on a face of the box, saved once; the same design with a cut `Extrude` in place of the join, saved once. For each save, the board of the last state compared slot by slot with the records of the box in the state before it: which faces, loops, coedges, edges, vertices, points and carriers keep their slots, and which the board deletes.

### DR-884. Sketch placement without a tracked identity

**Question.** What does a sketch placement path name when it references no tracked entity identity?

**Known.** `f3d.md` §3.1 "**Sketch placement.**" and "**Placement path.**" give the path of a sketch on an origin plane or a construction plane, of type `5A1BF548-241F-46FD-9FB5-E4B05126EB9D`. A placement can also reference a path of type `5662F619-2281-4AD9-93DC-1FCFDA1062E7`.

**Need.** The placement of a sketch on a body face.

### DR-886. Item-tail companion word

**Question.** What does the u32 at offset 29 of the item-tail companion (type `5FBE538E-139B-46D4-BCAF-CC796D1E1F78`) encode?

**Known.** `f3d.md` §3.1 "**Item-tail companion.**" gives the record. The word is `0`, `2`, or `3`. A `Sketch` or `Extrude` item with `s0` and `s1` equal to `0` can store `0` or `2`. Most `Sketch` items store `0`. The placement and the placement path of a `Sketch` item each reference an item-tail companion too. A `Base Feature` item stores `0`. In a design whose timeline lists `Sketch` items and one `Extrude`, the start record, profile group, and profile selection of the `Extrude` each reference their own companion, which stores `0`.

**Need.** A writer must choose the word for each item it writes.

### DR-887. Face tokens of later Base Feature bodies

**Question.** At which token does the face-token run of the second and each later result body of a `Base Feature` start?

**Known.** `f3d.md` §3.1 "**Names in the B-rep of a Base Feature.**" gives the face tokens in each body and the start of the first body. In a design whose timeline holds only one `Base Feature` item and which has several result bodies, the first face token of a later body is the first face token of the body before it, plus that body's face count and edge count, plus `1`. Where a design's bodies carry a `Base Feature` identity but its timeline holds other items, the gap between one body's run and the next is mostly `0`, sometimes `1`, and sometimes neither, so no rule is established. Body ordinals and first face tokens increase in the order of the bodies in the B-rep stream. Some designs also tag edges and vertices of a result body.

**Need.** A writer of several result bodies must number the faces of each body. The edge and vertex tags of the earlier bodies against the start of the next run would settle it.

### DR-901. Sketch item bytes outside the stated rules

**Question.** What do the varying flags, words, names, and record forms of the records of a `Sketch` item encode, how does Fusion assign dimension suffixes, relation ordinals, and point and curve identities, and what does a persistent-id table store for `Sketch` items followed by a `Base Feature`?

**Known.** `f3d.md` §3.1 "**Sketch container.**" and the paragraphs around it give the records of a `Sketch` item with their fixed bytes. These fields vary across sketches whose other fields agree: the scope's property-block presence byte `P` and flag `F`; the container's u32 after its dimension-frame run (`0` or `1`) and its sketch word (`6` or `7` in the `EntityGenesis` form); the container's u8 after its sketch word; the point flag bytes 1, 4, and 6 and the point state byte; the curve flag `A`; the `EntityGenesis` value of a relation (absent, `0`, `2`, `4`, `32`, `128`, or `1024`), of a point (`0`, `2`, `8`, `258`, `4096`, or `4098`), and of a line or circular curve (`0`, `2`, `4`, `8`, `128`, `130`, `256`, `258`, `512`, or `4096`); the point selector (`0`, `1`, `2`, or `4`); the three bytes of the placement tracking flag; and the dimension owner's u64 parameter number, which equals the `n` of the parameter name `d<n>` in most owners. Most `Sketch` scopes store the family name `Sketch`, and other scopes store a localized family name. Most container entity names are `0_` followed by the decimal record index of the container, and other names start with `1_`. Most placement tracking flags store `01 01 00`. Most points store no four zero bytes before the sketch container reference. Most circular curves store the normal `(0, 0, 1)`, and another circular curve stores another normal. Most 42-byte line tails store zero in their three pad bytes and in their twelve bytes before the f32 `1.0` values; other 42-byte line tails store `01` in their third pad byte and the f32 `0.4` in those twelve bytes. A line's curve tail is 44 bytes after one point reference and 42 bytes after two or three in the common form, with zero pad bytes; some lines store a 46-byte or 53-byte tail, whose extra 4 or 11 bytes may be a reference field. The first run of a container also lists relations of types other than `60403D47-0C49-49B0-BDE8-1679608164A2` and `24DB790E-3DCD-4336-AFA3-6F119EF2239B`. Curve flag `A` mostly equals flag byte 4 of the curve's end points. Most origin points store the flags `00 00 00 01 01 01 00 00` and state `0`; other origin points store other flags or another state, and some of them keep state `0` and clear flag byte 4. A dimension's four records are either frame, parameter, owner, companion or frame, owner, parameter, companion, and one sketch can use both arrangements. Their record indices are mostly consecutive. In most sketches the source-kind suffix of the `j`-th dimension of one kind in prefix order is `j + 1`, so the first linear dimension is `Linear Dimension-2`; other sketches store other suffixes. A member's relation ordinal is mostly the number of relations and dimension frames with a smaller record index that name the member; other ordinals are larger, and an ordinal can be smaller. In most sketch collections, the `pt_tag` of the points and the `crv_primary_id` of the curves are distinct, start at `100`, and give the origin point of the first sketch identity `100`. In most sketch collections, `crv_tag_id_idx` is one more than the largest identity of the sketches it lists, and in the others it is larger. A design whose timeline holds only `Sketch` items lists its `Sketch` identities in the pairs and none in the forward run; a design with one `Base Feature` lists it in both.

**Need.** A writer must choose each of these bytes, suffixes, ordinals, and identities, and must write the persistent-id table of `Sketch` items before a `Base Feature`. The flags against the sketch's solved state and the constraints that hold each entity, the owner's parameter number against the creation order of the design's parameters, the suffixes, ordinals, and identities against the deleted dimensions, relations, points, and curves of the sketch, and a design whose timeline holds `Sketch` items and a `Base Feature` would settle them.

### DR-910. Cross-segment references in the records of a Sketch item

**Question.** Which reference fields of the records of a `Sketch` item can name a record of another segment, which segment does each such reference name, and when does Fusion write the fifteen-byte form?

**Known.** `f3d.md` §3.1 "This subsection gives the records of one `Sketch` timeline item" gives the record sizes and offsets of a record whose references are all same-segment references or null references, and a fifteen-byte reference moves every later offset of its record by four bytes. Scopes, placements, placement paths, sketch containers, points, relations, dimension frames, and dimension owners of `Sketch` items store fifteen-byte references.

**Need.** A writer must choose the form of each reference it writes. The field that holds each fifteen-byte reference, and the segment it names against the segments of the design, would settle it.

### DR-917. Variable fields of a sketch dimension frame

**Question.** What selects the relation ordinal of a dimension frame's null locus, the fourth point of a linear measurement, the origin and the axis signs of the text frame, the byte before the text frame, the style values of a frame with style byte `0`, the two f64 values of the linear kind block, the flag byte of a diameter measurement, the second quadrant word of an angular measurement, the order of the two point positions of each line of an angular measurement, the number of decimals and the unit of a driven dimension's text, and the family discriminator of a dimension's parameter? Which boundary combination does a linear dimension between two circular curves measure, and which dimensions do the kind masks `0x1000000` and `0x800000000` name?

**Known.** `f3d.md` §3.1 "**Sketch dimensions.**" gives the frame grammar. In most sketches, the null-locus ordinal of the `j`-th frame of the container's dimension-frame run, from `0`, is `c + j` with one constant `c` for the sketch. `c` is `3` in most sketches and another value in others, and `c` can increase between two frames of one sketch. The fourth point of a linear measurement is the first anchor `A1`, the text-frame origin, or another point. The text-frame origin is `(0, 0, 0)`, the fourth point of a linear measurement, the centre of a radial or diameter measurement, a point position of an angular measurement, the common centre of the two circular curves of a linear measurement, or another point. The byte before the text frame is `1`, and `0` in some offset and radial frames. The style values of a frame with style byte `0` are `(0, 0, 0, 1)` in most frames and other values in others. The two f64 values of the linear kind block are `(0, 1)` in most frames, and `(1, 0)`, `(1, 1)`, `(0, 0)`, or `(0, -1)` in others. The flag byte of a diameter measurement is `0` in most frames, and `1` or another byte value in others. The second quadrant word equals the first in most angular frames, and the first word selects the angle. Most angular frames store each line's end point before its start point, and some store the start point first. The text of a driven dimension has 2, 3, 6, or 7 decimals, and its unit name is `mm` or `deg`, the unit of its parameter's literal. The parameter of a sketch dimension is in the discriminated form with an owner reference; its family discriminator is `6`, `0`, `4`, `5`, or `3`, one value in most Design segments. A linear dimension between two circular curves with centre distance `d` and radii `r1` and `r2` measures `d + r1 - r2` or `d - r1 + r2`. The masks `0x1000000` and `0x800000000` occur in linear frames that measure one circular curve of type `FF23079A-D99C-47AB-940E-2F4E18F022AB`. In some linear frames between two lines, `A1` lies on the second measured line and `A2` on the first.

**Need.** A writer must choose each of these values. The null-locus ordinals against the deleted frames and relations of the sketch, the text-frame fields and the fourth point against the text placement of the dimension and the normal of the sketch, and the style, flag, and kind-block values and the decimals and unit of a driven text against the display settings of the dimension and the document would settle them.

### DR-918. Varying bytes of the serializer-trigger, Scene and settings records of a Base Feature design

**Question.** Which bytes of roster records 185 (`49B2AA69`, configuration-table serializer trigger) and 186 (`3185B924`, configuration-rule serializer trigger), and of the roster's Scene and settings records, differ between one-body `Base Feature` designs, and what selects each value?

**Known.** `f3d.md` §3.1 "**Record roster.**" gives the type, version and length of each of these records. Apart from record indices, class tags, GUIDs and names, they differ between one-body designs at offsets not yet located.

**Need.** The offsets at which they differ and what each value holds, so that a writer states them instead of taking the bytes of one design.

### DR-912. Count before the parameter-collection companion

**Question.** What does the u32 `c` at offset 30 of the parameter collection (type `B0556F58-EE09-46A7-B18F-E296F48E5B13`, type version `4`) count, and what is the layout of a collection that stores nonzero bytes in the run that the layout gives as zero?

**Known.** `f3d.md` §3.1 "**Component records.**" gives the layout. `c` is at least the parameter count `n` and often equal to it. A version-4 collection can store nonzero bytes in the zero run after its indexed header; such a collection is longer than `61 + 11n` bytes.

**Need.** A writer stores `c = n` and the zero run. Collections with `c > n` against their deleted or unused parameters would settle the meaning, and the parameters of a collection with a nonzero run against its bytes would settle its layout.

### DR-899. Appearance entries of a body-reference record

**Question.** What do the appearance entries of a body-reference record (type `D3937028-C20C-4E65-B010-94AD418A5C20`, type version `19`) select, and what is the layout of an entry?

**Known.** `f3d.md` §3.1 "**Body-reference records.**" gives the record with entry count `k = 0`. A design and a parameter body-reference record of one body store `k = 2`. Each of their entries is 294 bytes and is the legacy per-face appearance assignment entry of `f3d.md` (the paragraph that begins "Legacy per-face appearance assignments"): an LP-UTF16 GUID of 36 code units, four f32 colour components with alpha `1.0`, bytes `01 01`, nine zero bytes, u8 selector-name form `1`, an LP-UTF16 visual token, the library `BA5EE55E-9982-449B-9D66-9F036540E140`, u32 `0`, the selector `PrismOpaque`, f32 `0`, and f32 `1.0`. The entries follow the u8 at offset 292 and the u32 `k` at 293. The two entries store different GUIDs and colours. The fields after the entries are the fields of the `k = 0` form that follow the count.

**Need.** A writer of a body with more than one appearance must write the entries. Bodies whose faces carry appearances against their entry GUIDs and colours would settle what an entry selects.

### DR-913. Evaluation duration and epoch of an item tail

**Question.** What unit does the evaluation duration of a feature item tail use, and what selects the evaluation epoch?

**Known.** `f3d.md` §3.1 "**Record timestamps and evaluation values.**" gives epoch `252` for every item of a stream with MetaStream generation words `1234, 8, 36`, and a positive duration for a `Base Feature` item. Streams with other generation words store other epochs. The streams with generation words `1234, 8, 20`, `1234, 8, 23`, and `1234, 8, 27` each store more than one epoch value.

**Need.** A writer must choose both values. The relation between the epoch and the generation words in the other streams.

### DR-896. Fitted extents of the `HOME` camera

**Question.** What rule gives the view extents `w0`, `w1` of the `HOME` camera and the scene radius at offset 426 of the `AssetSettings` record, and what centre does the camera use in a design with more than one result body?

**Known.** `f3d.md` §3.1 "**Named views.**" and "**`AssetSettings`.**" give the camera layout, the target, the view direction `(1, -1, 1)`, the up direction, the field of view `2 atan(0.2)`, and the eye distance `2.5 max(w0, w1)`. `w0` is between 1.08 and 1.11 times the height, and `w1` between 1.07 and 1.09 times the width, of the body's bounding box projected on the view plane. The scene radius is between 1.0002 and 1.0044 times half the diagonal of that box. In a design with three result bodies, the target is not the centre of the union of the body boxes.

**Need.** A writer must store the extents, the radius, and the target. The displayed tessellation of the body, projected on the view plane, against the extents would settle the rule.

### DR-914. Layout of the OGS display scene

**Question.** What is the layout of the members of `OGS.BlobFolder/OGS/DefaultScene/` (`world`, `stream_mesh_<n>`, `Fusion_mesh_<n>`), and what selects the numbers `<n>` in their names?

**Known.** `f3d.md` §3.1 "**Archive members of a Base Feature design.**" lists the members. `world` is a serialized scene graph of UTF-16 type names (`ARenderList`, `GroupNode`, `GeometryNode`, `OgsSimpleMesh`, `VertexFormat`, `AIndexBuffer`, and the attribute types) with origin, edge, face, and body nodes; it stores pointer-like hexadecimal node names and the Protein instance GUIDs of the body. `stream_mesh_<n>` is 860 bytes with the same bytes in every one-body solid design. `Fusion_mesh_<n>` members store f32 vertex data.

**Need.** A writer must decide whether to store the display scene and, if so, its bytes.

### DR-915. Records and snapshot stream of a design with several or sheet result bodies

**Question.** How does a `Base Feature` design with several result bodies, or with a sheet result body, allocate the record indices of its roster, and what values do its snapshot body-map companions list? How does the snapshot stream of a design with several result bodies differ from the solved part of its history stream? What sets the first value of the design snapshot body-map companion of a sheet body?

**Known.** `f3d.md` §3.1 "**Record roster.**" gives the ten records that each further solid body adds. In a design with three solid bodies, the further design body-reference records and their Scene records follow record 192, the further parameter body-reference records and Scene records follow record 222 with unused indices between them, and the further tracking entries and browser-node members follow. Its snapshot stream has the same topology counts as the history stream, and geometry records of the two streams differ. In a design with one sheet body, whose faces, edges, and vertices carry generic-tag attributes, the design companion's values start 4 above the parameter companion's next value.

Record indices below are the indices of the roster of a design with one solid result body. In a design with one sheet body, the alternate `Body` owner is record 193 and every later record moves up one index. Records 194 and 196 are records 195 and 197, with the same bytes apart from their indexed headers. The bodies root names records 187 and 216. Browser record 16, and the browser root's entry-map pair for it, name the parameter `Body` owner, record 194.

In a design with three solid bodies, records 193 to 200 are the further design body-reference records and their Scene records, and records 193 to 222 move up eight indices. Records 194 and 196 are records 202 and 204, with the same bytes apart from their indexed headers. The bodies root is 116 bytes and names records 187, 193, 197, 223, 231, and 238. Browser record 16, and the browser root's entry-map pair for it, name the parameter `Body` owner, record 201. The parameter companion lists one run of `F + E` consecutive values for each body, in body order, from `0`, with one unused value after each run; its next value is the value after the last unused value. The design companion lists the `n` values that follow, from that next value.

**Need.** A writer of several bodies or of a sheet body must allocate the indices, list the companion values, and write the snapshot stream. Designs with two and with more than three result bodies, and with several sheet bodies, would settle the allocation, the companion runs, and the geometry difference.

### DR-900. Alternate forms of Base Feature roster records

**Question.** What selects the alternate forms of the design `Body` owner and the root instance, and what sets the next value of the parameter snapshot body-map companion?

**Known.** `f3d.md` §3.1 "**`Body` owners.**", "**Root instance.**", and "**Snapshot body-map companions.**" give the common forms. A design `Body` owner can store byte `1` after its ten zero bytes, followed by u32 `0`; its later fields then start four bytes later, and in a design with a sheet result body its first reference run names the alternate `Body` owner. A root instance can store a property list of count `3` with the properties `GatedByParent` (`IntrinsicMetaTypebool`), `ResolutionTime` (`IntrinsicMetaTypeHString`), and `SelfGated` (`IntrinsicMetaTypebool`), each an LP-ASCII name, an LP-ASCII value type, and the value; that form is 384 bytes. The parameter companion's next value is `n + 1`, `n`, or `n + 3` for `n` listed values.

**Need.** A writer stores the common forms and the next value `n + 1`. The save conditions that produce each form would settle the selection.

### DR-888. Nonzero one-byte ordinal prefix of an attributed local occurrence

**Question.** What does a one-byte ordinal prefix with value `1` select in an attributed local component occurrence?

**Known.** `f3d.md` §3.1 "A version-two attributed local occurrence also permits a one-byte zero ordinal prefix." defines the zero prefix. A class-`416` carrier in an inserted document stores prefix byte `1` and u32 ordinal `1`, followed by the carrier GUID and the occurrence key. The rest of its frame has the zero-prefix layout: a null identity reference, the repeated component reference, identity flag `1`, nine zero bytes, and the owner entity. A displayed-pose path names this carrier in its final pair under each insertion of the document. The decoder rejects the frame. The component definition then has a synthesized identity placement, and the displayed-pose rows of the carrier stay unbound with `scene.override-unbound`.

**Need.** Saves that change only this prefix byte while the occurrence and its placement stay fixed, or a writer-side definition of the byte.

**Conflict.** None.

**Note.** Each carrier GUID, occurrence key, owner entity, and component reference of the frame resolves. The prefix value is the single field outside the specified grammar.

### DR-911. Fields of a sketch-driven Extrude without an established rule

**Question.** For a sketch-driven `Extrude`, what does the byte at offset 27 of the scope frame encode, and what selects the family discriminator of its parameter records, the body key of its body map, the order of its five scope references, and the 107-byte form of its `Body` owner?

**Known.** `f3d.md` §3.1 "**Sketch-driven Extrude scope frame.**", "**Extrude parameter owners.**", and "**Result body.**" give the records. The table `sketch_extrude_scope_frame` in `docs/layouts/f3d.toml` records the byte at offset 27 as a gap. The byte is `0` or `1`, and one design can store both values. The value `1` occurs with positive and with negative `AlongDistance` values. The two parameter records of one `Extrude` store the same family discriminator, and the values `6`, `4`, and `0` occur. The parameter records of a Design stream whose first component naming-space record is record `6` store family discriminator `6`, document parameters included. In a stream whose first naming-space record is record `3` or `4`, the values `0`, `3`, `4`, `5`, and `6` occur, one stream can hold several of them, and its document parameters store a value that its owned parameters also store. The body key is `0` or `6`. The five scope references occur in the order of the list in `f3d.md` and in the order `AlongDistance` owner, `TaperAngle` owner, start record, profile group, profile selection. The `Body` owner also has a 107-byte form, with a different zero run and different words at offsets 22, 41, and 64.

**Need.** A writer must fill each field and choose each form. A design that stores both values of the scope byte, compared item by item against its edit history, would settle the first.

### DR-919. Member order of the owner collection and dependency table of a sketch-driven Extrude

**Question.** In a design whose timeline lists one `Sketch` and then an `Extrude`, in what order do the owner collection, the record of type `B65B5682-9355-44C7-8AFC-823058C4A36B`, and the dependency table (type `B13113CC-031E-4F93-8240-C4C10728869F`) list their records, and what adds a record of type `BFB592FE-03DD-4DAA-A4AC-14D02ABB672A` to them?

**Known.** `f3d.md` §3.1 "This subsection gives the records of an `Extrude` item that creates a new solid body" gives the records. The three lists each name the same ten records. In the common form, the owner collection lists ten members in this order: the record of type `5D15E992-1E93-4D79-A553-10D2624148AE`, the `Sketch` scope, the placement path, the sketch placement, the profile group, the profile selection, the `AlongDistance` owner, the `TaperAngle` owner, the start record, and the `Extrude` scope. The `B65B5682` record lists them in this order: the `5D15E992` record, the placement path, the sketch placement, the `Sketch` scope, the profile selection, the profile group, the `AlongDistance` owner, the `TaperAngle` owner, the start record, and the `Extrude` scope. The input map of the dependency table stores these entries in this order, each with the records it reads and their flags: the `Sketch` scope reads the `5D15E992` record (`0x1000`) and the sketch placement (`1`); the sketch placement reads the placement path (`1`); the `Extrude` scope reads the `5D15E992` record (`0x1000`) and the start record (`1`); the profile group reads the profile selection (`1`); the profile selection reads the `Sketch` scope (`4`); and the start record reads the profile group (`1`), the `AlongDistance` owner (`1`), and the `TaperAngle` owner (`1`). The dependent map stores the same edges by the record read, in this order of read records: the `5D15E992` record, the `Sketch` scope, the placement path, the sketch placement, the profile group, the start record, the profile selection, the `AlongDistance` owner, and the `TaperAngle` owner. The three lists of some designs also hold one record of type `BFB592FE-03DD-4DAA-A4AC-14D02ABB672A`.

**Need.** A writer must write the three lists. The position of the `BFB592FE` record in each list, against the edit history of the designs that hold it, would settle the question.

### DR-916. Records of a sketch-driven Extrude in a design with further items

**Question.** In a design whose timeline holds items other than `Sketch` items and one `Extrude`, which records do the record-timestamp tables of a sketch-driven `Extrude`'s records list, which presentation records does its result body hold, which coedges carry a `sketch_attrib_def` attribute, and which persistent identity does a later `Sketch` scope own?

**Known.** `f3d.md` §3.1 "This subsection gives the records of an `Extrude` item that creates a new solid body" gives the records. In a design whose timeline lists `Sketch` items and one `Extrude`:

- The record-timestamp table of the `Extrude` scope lists the record of type `5D15E992-1E93-4D79-A553-10D2624148AE`, the `Extrude` scope, and the start record. The table of the start record lists the profile group, the `AlongDistance` owner, and the `TaperAngle` owner. The table of the profile group lists the profile selection. The table of the profile selection lists the `Sketch` scope.
- The body-presentation owner is the only presentation record of the result body.
- `f3d.md` §3.1 "**Sketch links of the start cap.**" gives the `sketch_attrib_def` attributes on the edges of the start cap and of the end cap. The other coedges of the side faces carry no `sketch_attrib_def` attribute.
- When further `Sketch` items follow the `Extrude`, each later `Sketch` scope owns the next persistent identity after `302`.

**Need.** A writer of a design with further items must fill these tables, presentation records, attributes, and identities. Designs whose timeline holds several `Extrude` items or other features, compared record by record with these lists, would settle it.

### DR-1049. Records after the profile group of a written sketch-driven Extrude

**Question.** Does Fusion read a sketch-driven `Extrude` whose profile group `G` is followed by its identity wrapper, its item-tail companion, and its record-timestamp table, in that order?

**Known.** `f3d.md` §3.1 "**Profile group.**" gives the profile group of a sketch-driven `Extrude`: a construction-operand group of type `2CA5A1CD-C99B-4C9B-91AF-57989148E841`, type version `1`, that references its 24-byte identity wrapper (type `2DF7DA30-A4E4-4260-AFD1-D9C7154CB44B`), its item-tail companion (type `5FBE538E-139B-46D4-BCAF-CC796D1E1F78`), and its record-timestamp table (type `6CCF41D5-40BE-48ED-A834-18F3EAED6C57`). In a stream with serializer magic `1234`, the construction-operand groups of `Extrude` scopes and of `Revolve` scopes store the record-timestamp table as record `G + 1` and the item-tail companion as record `G + 2`. The identity wrapper is neither of these two records. `f3d.md` §3.1 "**Revolve groups.**" states this order for the groups of a `Revolve`. The decoder's construction-operand group reader closes a group only when the group's reference to its record-timestamp table names record `G + 1`; it reads the ordinary form when the reference to the item-tail companion names record `G + 2`.

**Conflict.** The writer allocates the profile group of a sketch-driven `Extrude` as `G`, its identity wrapper as `G + 1`, its item-tail companion as `G + 2`, and its record-timestamp table as `G + 3`. The decoder leaves the group of a written `Extrude` unclosed. The written `Extrude` decodes without the group, because its projection does not read the group. The writer allocates the groups of a `Revolve` in the stored order.

**Need.** Allocate the profile group of a written `Extrude` as the groups of a `Revolve` are allocated: the record-timestamp table at `G + 1` and the item-tail companion at `G + 2`. Then state the relation in `f3d.md` §3.1 "**Profile group.**", and compare the records of each written `Extrude` before and after the change. The same comparison of the `Extrude` start record against stored `Extrude` start records would show whether its record-timestamp table and item-tail companion also belong at the next two records.

## 2. External references

### DR-111. Component-path poses of nested occurrences

**Question.** How does a component-path record with more than two path elements join to a nested occurrence, and does its world pose supersede that occurrence's own placement?

**Known.** A two-element path of a cross-document element and a local element naming an occurrence-placement record stores the current world pose of that placed occurrence. A two-element path of a Component record and a local occurrence carrier of that component stores the current pose of a local occurrence in the root component. Longer paths continue through further cross-document and local elements, each with an instance discriminator, and also store a world transform. A path of a referenced design's root component and occurrence placement, then this document's placement of the external occurrence and some of its local owners, stores the world pose of the inner occurrence; when the records of one instance that list the most owners agree, they place it. A path of a referenced design's root component, then this document's placement of an external occurrence and one or more of its local owners, stores that external occurrence's world pose; it places an instance that no contained record places. An inline-qualified own path places its insertion independently of the contained carriers that identify child instances. A full-owner nested carrier can store origin `(75, -124.5, 450)` mm while its own-root carrier stores `(80, -125, 450)` mm, and its same-role contained-body carrier stores `(221.330802, -259.941433, 443.85)` mm. The class-301 pathless placement binds the nested carrier to its target and stores a local translation of `(0, 0, -0.5)` mm.

The first closing reference run identifies a direct contained child by its local insertion placement and cross-document target, in that order. Nested-design carriers can share their insertions with ordinary contained carriers and name distinct external child instances by this chain. The contained carriers of an inserted design can equal the product of the stored insertion placement and the direct source child pose while their insertion's own current carrier states another insertion frame. The source composition preserves the child local pose under that current frame.

**Need.** Why an external occurrence's own two-element record, and a contained record that lists fewer owners, can keep an earlier pose while the records that list more owners give another; the join from each later path element to the placement record of the nested occurrence in its placed design, and the relation between that world pose and the nested occurrence's own placement and joints, and whether a two-local-element pose of an occurrence below another component is local to its container or in the world. The saved-state selector that relates a full-owner nested path to the own-root and same-role contained paths.

**Conflict.** None.

**Note.** The contained and own forms above place occurrences (`f3d.md` §1.4). Other longer component-path records do not place occurrences; their bytes stay in the source stream.

### DR-113. Solved placement without a bound assembly operand

**Question.** Which native relation, if any, selects the current solved pose of an inserted occurrence when no assembly operand path binds to it?

**Known.** `f3d.md` §1.4 defines stored occurrence placement and component-path pose precedence. Assembly operand paths can select occurrences whose solved poses differ from their stored transforms. Operation-table type `549FBB80-B890-473E-A5C0-415D3D9BF4E6` has version-0 and version-1 forms. A typed one-element path identifies the affected occurrence. A parent descendant-path table can equal a child table's final Position matrix while the child's stored matrix equals its insertion entry. The direct table's final entry is a property of the occurrence inside its component definition, common to every instance. Component-path world poses constrain the containing instance. An instance-specific edit of the shared child violates this relation. The direct version-0 and version-1 grammar and precedence are in `f3d.md` §1.4.

**Need.** Bind each operation-associated transform to its typed occurrence path, coordinate frame, and saved state. Identify the saved-state selector that establishes whether a displayed world pose or an operation-associated transform is the active joint solution. Establish its precedence relative to occurrence-placement and component-path transforms. Distinguish a contained placement change from a containing placement change. Identify the operation suppression state and saved timeline marker before selecting an entry. Independently authored saves must change one placement or one suppression or timeline state and retain the complete occurrence, operation, and constraint carriers.

**Conflict.** A stored placement can disagree with the checked assembly while no projected joint selects the occurrence. This does not distinguish an unbound constraint, an unread placement carrier, and a different saved assembly state.

**Note.** `f3d.md` §1.1.4 "The prefix is followed by a counted pointer string" defines both uniformly tagged and untagged display paths. A complete display-path binding identifies the occurrence and its displayed world frame. The root, local, contained, and owned component-path forms in `f3d.md` §1.4 keep their precedence. The remaining Need applies to other path forms, incompatible constraints, and saved-state selectors (DR-111). Direct one-element tables do not select suppression or an active timeline marker. A definition with incompatible component-path and table constraints keeps its stored pose for every instance; the operation table stays native. Other path forms and saved-state selectors remain unresolved.

### DR-109. Legacy external placement control and extended envelopes

**Question.** Which native objects do the auxiliary placement UUID and cross-segment metadata reference identify? What does the control word select, and how do change-category lists delimit the placement?

**Known.** `f3d.md` §1.4 defines the compact and attributed placement envelopes, their occurrence roles, and their local owners. A change-category envelope contains the ordered names `Appearance`, `Geometry`, `PhysicalMaterial`, `Properties`, `StructureChange`, and `Transform`, local references, and a local occurrence name before further placement members.

**Need.** A typed join through the metadata reference's segment namespace, the auxiliary UUID's relation to that target, the control word's effect on placement, and the complete grammar of the change-category envelope.

**Conflict.** None.

**Note.** Auxiliary metadata remains in the complete source frame with `xref.placement-metadata-native`. Extended control and change-category envelopes remain `xref.placement-undecoded` losses.

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

**Known.** `f3d.md` "Assembly operands" gives the joint and motion records, the freedom entry, codes `1`, `2`, and `8`, and the limit lanes. Freedom codes `0`, `6`, and `7` also occur, alone and in pairs such as `2` with `7`, `0` with `8`, and `2` with `6` and `7`. Both flags are `1` on a joint with a minimum and a maximum. A slide whose joint record stores −0.5 and 1.6 has `SlideMinimum` and `SlideMaximum` owners of −5 mm and 16 mm, so translation limits are in centimetres. The 444-byte `As-built` form adds `SlideMinimum`, `SlideMaximum`, and `SlideRest` owners at local ordinals four, five, and six. Where both a `SlideRest` owner and a joint-record rest value are stored, both are `0.0`, so the stored values do not show whether the owner holds the rest value. In a 444-byte scope, the `SlideRest` owner stores f64 `0.0` cm, and the linked joint rest lane stores the equal value f64 `0.0` cm (`0.0` mm). The joint rest flag is `1`.

**Need.** Cylindrical, pin-slot, planar, and ball joints combine freedoms. A joint with only one limit needs the flag that enables it.

### DR-83. Attributed occurrence members

**Question.** What do the two bytes before the occurrence ordinal and the marked record reference after the occurrence key of an attributed local component occurrence store?

**Known.** `f3d.md` §3.1 "An attributed local component occurrence carries a counted attribute run and an occurrence key." gives the frame. The two bytes are `00 00` or `01 00`, and `00 00` is the common value. The reference of each frame names a different record, and the named records increase in the order of the frames.

**Need.** The two bytes can be a grounding state of the occurrence. A controlled attributed occurrence must identify the marked source reference when its target has no type registration.

**Conflict.** None.

**Note.** The decoder reads an unregistered source as a component occurrence and reports `assembly.local-occurrence-source-unregistered`.

### DR-84. Occurrence identity lists of a typed path

**Question.** What do the occurrence identity lists after a typed occurrence path's identity GUIDs name?

**Known.** `f3d.md` "Assembly operands" gives the lists. The paths of several joints of one assembly can carry the same single list of two keys: a decimal occurrence key and an occurrence GUID that name two different local occurrences. The two occurrences differ from the joint's own operand pair. The paths of other joints of that assembly, and a path continuation, carry no lists. A list can also hold three or four occurrence GUIDs, and the leading keys of a list can equal the leading occurrence GUIDs of the path that stores it.

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

**Known.** `f3d.md` §3.1 "The paired untyped Body-container membership" requires registered Body and Component targets. A registered version-4 Body container has a paired untyped terminal list that names two Body entities and one owner entity. The owner has type `074F9459-89C2-4B7D-8AB4-DCE67AE944FE`, base type `63D6AD42-466A-40B7-9CCD-EB21BC500EF6`, version 1, module `Fusion`.

**Need.** A typed reference chain from this owner to a Component definition, with the containing record boundaries and registrations.

**Conflict.** None. The Fusion owner is outside the direct Component-owner grammar.

**Note.** The body identities and terminal list are defined; their component-definition join is unresolved.

### DR-116. Joint-placed identity occurrences of inserted designs

**Question.** Which native relation binds a joint to an inserted-design occurrence that stores the identity and has no component-path carrier that states its pose, and in which frame does that joint define the solved pose?

**Known.** `f3d.md` §1.4 "The matrix is a proper rigid motion." defines stored identity placement and the component-path world-pose carriers. A joint names an occurrence directly through an occurrence container, or names an inserted design through an external operand container, leaving the neutral tie unresolved; each distinct external document then accounts for at most one unnamed identity occurrence. The decoder counts these occurrences, reports `assembly.joint-placed-occurrence-unsolved`, and places them at the identity with the joint unsolved. The decoder places root occurrences that have carriers of a nested design from those carriers, also when they store the identity placement, and removes them from the count. The root `As-built` scopes with a root operand path can name none of these occurrence identities. An identity occurrence can have no carrier; the occurrences inside it then keep one world pose in its insertion scope and in every captured position. A root display scene can store the solved pose of such an occurrence: for a fixed joint whose first operand path names an inserted identity occurrence and a child inside it, the scene's world matrices of all child occurrences equal the pose that the Assemble connector equation gives, with the operand frame in the child's space. The negated angle gives no match.

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

### DR-904. Protein stream header words

**Question.** What do the two i32 words and the u32 word at bytes 4 through 15 of a Protein stream header record?

**Known.** `protein.md` §2 "`InstanceProperties.bin` and `DefinitionIteratorProperties.bin` start with" gives the values `-1`, `-1`, and `0`. Every stream holds consecutive records with no page outside a record (`protein.md` §3).

**Need.** A stream whose pages include a page outside every record, or a stream that a producer extended in place, to relate the words to page allocation.

### DR-905. Text of the Protein schema documents

**Question.** Which text do the nine schema documents of a Fusion Protein package hold, and does a reader take the property declarations from the package or from its own library schema of the same identifier and `SchemaVersion`?

**Known.** `protein.md` §8 "A schema document declares one schema" gives the schemas, their bases, `type` values, and `SchemaVersion` values. `protein.md` §9 "The tables below give each instance record's value block" gives every serialized property with its declaring schema and carrier. A schema document also declares non-serialized properties, labels, bounds, user-interface and implementation members. The text of a schema document is the same in every package that has its identifier and `SchemaVersion`.

**Need.** The declarations that a writer must emit beyond the serialized properties, so that a package with generated schema documents reads the same as one with library schema documents.

### DR-906. Second form of the appearance of a Base Feature body

**Question.** What selects the second form of the body's appearance and its twin in the Protein package of a Base Feature design?

**Known.** `protein.md` §9 "The Protein package of a Fusion Base Feature design" gives the common form: the twin has base asset identifier `PrismOpaque` and the definition `Opaque(R,G,B)`, and the appearance has the colour `(246, 246, 243)`. In the second form, the twin is a copy of the appearance aspect: base asset identifier `PrismMaterial-022_physmat_aspects:Prism-374`, `AssetLibID` `C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C`, the aspect's values, and a definition record equal to the aspect's definition. The appearance then has `generic_diffuse` equal to the aspect's `opaque_albedo`, `generic_glossiness` `0.7015873193740845`, and `generic_reflectivity_at_0deg` `0.0529`. The other values of the body's appearance and twin, and the records of the physical material and its aspects, equal the common form. The packages of the second form share the physical-material identifier and the body-appearance identifier. `protein.md` §9 "An appearance and its twin carry one colour" gives the appearance pairs that a per-face assignment adds to a package of the second form: each pair has the common form with the colour of its face.

**Need.** The source property, for example a material or appearance setting of the design or of its template, that selects one form. A package of the common form with a per-face assignment, to show whether the form of an added pair follows the form of the body's appearance.

### DR-907. Join from an instance record to its definition record

**Question.** Which rule joins an instance record to its definition record when the definition's asset identifier differs from the instance's base asset identifier?

**Known.** `protein.md` §6 "`AssetData/AssetTableOfContents.bin` lists the instance records" pairs each instance record with one definition record by page index. In the package of `protein.md` §9 "The Protein package of a Fusion Base Feature design", the structural aspect has base asset identifier `PrismMaterial-022_physmat_aspects_1:StructuralPlastic-004` and definition asset identifier `StructuralPlastic-004`, the appearance aspect has base asset identifier `PrismMaterial-022_physmat_aspects:Prism-374` and definition asset identifier `Prism-374`, and the twin has base asset identifier `PrismOpaque` and definition asset identifier `Opaque(R,G,B)`. The physical material and the appearance have a base asset identifier equal to the definition asset identifier. The decoder (`crates/cadmpeg-codec-f3d/src/materials.rs`) joins an instance record to a definition through an asset identifier equal to the instance's base asset identifier. A schema-bearing instance joins through the pair `(asset identifier, schema identifier)` of `protein.md` §5 "A definition stream can repeat one asset identifier", and a schema-less instance joins when its asset identifier has one catalog schema. The joined definition supplies the schema identifier and the category, and repeats with differing categories supply no category.

**Conflict.** The equal-identifier join finds no definition for the two aspects and the twin, and the table of contents finds one for each.

**Need.** A decision whether the decoder joins through the table of contents when the package has one.

### DR-908. Members of other Fusion Protein packages

**Question.** Which rule gives the members of the Protein package of a Fusion design whose package differs from the package of a Base Feature design?

**Known.** `protein.md` §8 "The `.protein` member of a Fusion Base Feature design" gives the members of the package of a Base Feature design, in ZIP order. Other Fusion packages hold other member lists. Their `Schemas` folder adds schema documents, for example `PrismMetalSchema`, `PrismTransparentSchema`, `PrismLayeredSchema`, `StructuralMetalSchema`, `StructuralGenericSchema`, `ThermalCommonSchema`, `ThermalSolidSchema`, `BumpMapSchema`, `TextureMap2dSchema`, and `UnifiedBitmapSchema`, and can omit schema documents of the nine in §8. Some packages name the stream version member `AssetData/StreamVersionTable.inf`. Some packages add a `Resources` folder with `cnx.xml` and `core.xml`, and in it one folder per name of the form `{GUID}_Post2015`, with the suffix once or more, that holds `cnx.xml`, `core.xml`, and a `thumbnail` folder.

**Need.** The rule that selects the schema documents, the name of the stream version member, and the `Resources` members of a package, and the ZIP entry fields, text, and order of those members, so that a writer can emit the package of a design with other materials or appearances.

### DR-909. Separator of the `PrismMaterial-022` preview path

**Question.** What selects the separator `<s>` in the preview path of the `PrismMaterial-022` definition record?

**Known.** `protein.md` §9 "The table-of-contents entry of each instance names one definition record" gives the preview path `Mats<s>PrismOpaque<s>Presets<s>t_Prism-374.png`. The separator is `\` or `/`. Packages of the common form store either value. The packages of the second form (DR-906) store `\`.

**Need.** The producer state, for example the Fusion version or the material library that saved the design, that selects the separator.

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

### DR-645. Legacy sole-scope serializer membership and order

**Question.** Which native relations establish complete cage membership and order in the sole-scope legacy Form envelope?

**Known.** `f3d.md` §1.1.1 defines the legacy scope, cage group, metadata group, and serializer frames. An active serializer entry establishes its cage identity. The decoder assigns the active serializer set to the sole Form with `feature.form-legacy-serializer-membership-assumed` and reads serializer frame order with `feature.form-legacy-serializer-order-assumed`. Present resources without neutral cages retain incomplete membership.

**Need.** Controlled saves that vary session membership and cage order while retaining stale serializer records.

**Conflict.** None.

**Note.** Membership certainty and order certainty remain independent.

### DR-658. Conflicting and foreign Form reference joins

**Question.** Which Form cage belongs to a source slot when its body, serializer, tracking group or owned list carries conflicting type, owner, position or namespace evidence? What frames a sparse tracking run after an unreadable member?

**Known.** Form reference runs retain local, segment and document targets, inline and separate type claims, qualified asset text, foreign link data, nulls and source positions. A body surface joins a serializer through a compatible local target. Distinct source counts or positions can describe incompatible membership claims. An unreadable variable-width tracking member leaves subsequent slots unvisited. A matching fixed-slot envelope permits an independently retained alternative.

**Need.** Controlled saves that introduce a foreign component and vary each Form selection, body owner and serializer independently, with the producer's resolved cage membership and reference namespace recorded.

**Conflict.** A retained inline type can differ from the registered type of its local target. Separate source records can establish different positions or counts for a Form.

**Note.** `feature.form-reference-type-conflict`, `feature.form-foreign-reference-unresolved`, `feature.form-tracking-owner-conflict`, `feature.form-serializer-reference-unresolved` and `feature.form-membership-conflict` record the unresolved joins and membership claims. `feature.form-tracking-fixed-slots-assumed` records the alternative fixed-slot reading. `feature.form-tracking-run-unresolved` retains the decoded owner when the member count is unavailable or exceeds the frame. Compatible local sibling facts remain available.

### DR-325. Empty Form selection group

**Question.** What does a zero member count in a Form selection group mean for the cages committed by its scope?

**Known.** A Form scope can have a selection group with zero members and no Form body record that names the scope. The neutral Form retains the zero tracking count with unknown cage cardinality and reports `feature.form-membership-unresolved`. An explicitly counted owned cage list establishes an empty cage sequence.

**Need.** Controlled saves of empty and edited Form sessions that compare the selection group, body records, and committed cages.

**Conflict.** None.

**Note.** None.

### TS-06. Radial deleted-slot indices and nondefault sweep

**Question.** Does a radial operand address a raw program slot or a compact live-element index when its domain contains a deleted slot, and how does a nondefault sweep change the rotation?

**Known.** `f3d.md` §1.1.1 "In a domain with no deleted slots" defines the same-numbered neutral binding and selector-specific element domains. Raw-slot and compact-index predictions agree on all-live topology. An out-of-range operand supplies no discriminator. A raw pair remains retained when the predictions disagree. At sweep `1`, the forward and reverse rotation steps are `+2*pi/N` and `-2*pi/N`; the native sweep scalar remains separate.

**Need.** A producer-generated radial TSM must retain a deleted domain slot at `p` and map a live element with raw slot `q > p`. The raw-slot prediction is `q`; the compact-index prediction after one deletion is `q-1`. Saving must preserve the deleted slot to separate these predictions. A controlled radial export with a nondefault sweep must establish the sweep-to-angle rule.

### SM-02. Unfold controls, rule factor and Refold association

**Question.** Which authored controls establish the Unfold selection slots, all-bends flag, settings-reference layout, rule neutral-axis factor and Refold source association?

**Known.** The Unfold prefix contains a control byte and four ordered references. Both construction groups use role `0x10`. The projection reads the first group as bend faces, the fourth as the stationary face, and control zero/one as selected/all bends. `feature.unfold-selection-slots-assumed` and `feature.unfold-all-bends-flag-assumed` identify these readings. A settings primary can contain present input references before the rule. The rule index of the decoded settings record, whose owner reference names this Unfold, supplies the rule link. A unique registered rule target inside the bounded settings frame, whose closing local owner reference names this Unfold, supplies the rule link with `feature.unfold-settings-rule-reference-assumed` when no decoded settings record exists. The unique material-trailer scalar of the owning settings rule supplies the neutral-axis factor with `feature.unfold-material-scalar-factor-assumed`. The unique preceding typed Unfold in the same ordered GroupMode supplies the Refold source with `feature.refold-group-source-assumed`. A resolved source must be emitted earlier in the same stream. Missing or ambiguous source links, rule factors, prefixes, selections and control values retain neutral partial constructions with their specific unresolved or native-selection losses.

**Need.** Controlled edits of each selection, the all-bends setting, settings input presence and the rule factor, plus a group containing several Unfold operations, must establish these associations.

### SM-01. Bend line association and stationary role

**Question.** How does a Bend scope associate each angular owner with a selected sketch line and its stationary region?

**Known.** The scope's ordered reference table contains construction groups, their members, and `BendAngle` owners. A role-`0x10` group carries a face selection. `BendRadius` parameters carry explicit radius overrides. The projection associates each angular owner with the group two references before it and interprets the role-`0x10` group as the stationary face. `feature.bend-line-owner-order-assumed` and `feature.bend-stationary-role-assumed` identify those readings. Missing angular definitions and unresolved rule radii carry `feature.bend-angle-unresolved` and `feature.bend-rule-radius-unresolved`. Multiple explicit radii use the first value and carry `feature.bend-radius-first-assumed`. A `BendAngle` definition with an absent owner frame binds through its owner backlink and the preceding singleton role-`0x21` group/member pair with `feature.bend-angle-owner-backlink-assumed`. A scoped cached owner following that pair supplies radians when its definition is absent with `feature.bend-cached-owner-angle-assumed`.

**Need.** A controlled Bend with reordered sketch-line selections, independent angle changes, a changed stationary region, and a document-rule radius change.

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

**Note.** `feature.edge-flange-stored-radius-inside-assumed` records the reading of this control as an inside radius when an explicit scoped radius is absent. An explicit `BendRadius` value takes precedence and admits zero.

### DR-117. Current mixed face and distance extent payload

**Question.** Which reference assignments and orientation fields define a first-side face termination with a second-side distance, and what do target options zero and two select?

**Known.** The current seven-slot prologue can carry direction two, first-side extent two, and a payload second-side extent one. Target option zero also occurs with a payload second-side extent one or two. Target option two occurs with a payload second-side extent two. The final zero word before the scope reference count does not identify the second-side termination. A solid new-body form with profile-plane start, target option zero, and extent pair `[2, 2]` can name the first and second role-`0x0000000500000000` groups in zero-based slots six and four. The groups each have one member and occur in side order in the scope table. The offset references in the payload name `Side1Offset` and `Side2Offset` owners. The decoder reads target options zero and two with the side terminations of target option one and reports `feature.extrude-target-option-assumed`.

**Need.** Controlled face-then-distance changes with both direction states and independent side tapers.

**Conflict.** None.

**Note.** The scope and ordered references remain native.

### DR-877. Split tool extension in other property forms

**Question.** How is the splitting-tool extension Boolean encoded when the property block is absent or populated?

**Known.** The empty-property form places the Boolean before a u32 one and a nullable local reference. The absent-property form has the same lane after the presence byte. The decoder reads that lane as the Boolean and reports `feature.split-tool-extension-assumed`. An unrecognized prefix reports `feature.split-tool-extension-unresolved`.

**Need.** Matched extension-on and extension-off saves with absent and populated property blocks.

### DR-878. Split face-tool groups with alternative face recipes

**Question.** Which splitting surfaces do a role-`0x0000000900000000` group with multiple `[0, −1]` recipes and a single-member group with a `[0, 0]` recipe select?

**Known.** Each group member has a complete face-recipe frame and occupies its ordered scope-reference position. The decoder reads these members as splitting surfaces and reports `feature.split-body-face-tool-group-assumed`. A complete unique preceding-face reading projects the tool as a historical face selection. Other readings retain the native group.

**Need.** Controlled Split Body saves that distinguish the single-member terminal words and vary the number of splitting surfaces.

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

**Question.** What do the Emboss control words and native pairs select for profile direction, tangent chaining, auxiliary selections, and contour membership? How do the placement values map to the neutral Wrap operation?

**Known.** The prefix has one or two control words, a counted run of native pairs, four scalar-owner links, two selection-group links, an auxiliary-group link, and the common ordered reference table. The table indices are in increasing order; prefix references bind by record identity. A sketch entity link identifies a Sketch but does not identify its selected contours. The native tool maps positive Depth to emboss and negative Depth to deboss ([Emboss reference](https://help.autodesk.com/cloudhelp/ENU/Fusion-Model/files/SLD-REF-EMBOSS.htm)).

**Need.** Controlled exports that change profile direction, tangent chaining, auxiliary selection, contour membership, and each placement value independently; complete operand frames for the extended reference table.

**Conflict.** None.

**Note.** Project a neutral Wrap from each independently decoded scope. Prefer the parameter bound to the named scalar owner; a unique parameter with the corresponding source kind remains an independent reading when that association is unresolved, with a named scalar-owner assumption loss. Map a nonzero signed Depth to Emboss or Deboss by sign, with a named assumption loss while the control fields remain unassigned. Keep zero, missing, ambiguous, and invalid Depth values in an unresolved material mode. Interpret a missing or conflicting display-unit token through the scalar role's native centimetre or radian unit, with a named scalar-unit assumption loss. Retain unresolved profile and target links in native selections. Retain horizontal, vertical, and rotation parameters as native source properties and neutral parameters while their Wrap placement mapping remains unresolved.

### DR-120. Align operand roles and motion

**Question.** Does a form-four Align constructor matrix state the complete relative motion of its selected body group? What do its leading scalar, nullable reference slots, unsigned codes, auxiliary scope references, and omitted-matrix marker control? Which alignment geometry do the role-`0x0000000500000000` construction groups of the scope select? What do the codes and marked words of the eight-word constructor control?

**Known.** The Align prefix contains two references, an unsigned code, and a zero GUID. The references join to the ordered scope table. The compact and extended forms differ by one u32 word of value 1. The first reference selects the constructor. A version-two rigid-transform constructor stores source form four, a leading scalar, six nullable references, two unsigned codes, and a matrix-presence marker. Its row-major matrix has an orthonormal basis, a homogeneous final row, and translation in centimetres. The second prefix reference names the role-`0x0000000400000000` whole-body construction group at scope-reference ordinal zero. A one matrix-presence marker omits sixteen scalars and retains the common tail. The eight-word constructor form retains its marked words, codes, matrix scalars, separate scalar, and two scope back-references.

**Need.** Controlled Align exports that change object selection, source and target frames, reversal, and angle independently. An Align save that aligns one body to another supplies a body-alignment recipe.

**Conflict.** The stored matrix can describe a solved frame or a relative motion.

**Note.** Apply the form-four matrix to the selected body group with `feature.align-motion-assumed`. Treat an omitted matrix as identity with `feature.align-identity-assumed`. A scope with zero or several matching body groups projects an unresolved body selection with `feature.align-operands-unresolved`. Preserve the constructor fields and scope references natively. An unsupported motion reports `feature.align-motion-unprojected`.

### DR-144. Flag bytes of the discriminated parameter expression trailers

**Question.** What do the first four of the five flag bytes in the nine-byte and ten-byte expression trailers of a discriminated parameter frame state, and which of them is the driven flag?

**Known.** `f3d.md` §3.1 "The form of an indexed parameter record follows the version of its type" gives both trailers. Each ends with five flag bytes, each `0` or `1`, and the last is the document flag. The five flag bytes correspond position by position to the five-byte expression trailers of the version-5 compact forms: at each flag byte that a nine-byte or ten-byte trailer sets, parameters of the same source kinds set the same byte of the version-5 trailer. The first flag byte is set on some `TangencyWeight` feature inputs and on no other parameter. The first byte of the version-5 trailer, the opaque flag of the compact owned trailer, is set on some `TangencyWeight` parameters and on some driving dimension parameters. The fourth flag byte is set on dimension parameters with a null owner reference, and the fourth byte of the version-5 trailer is set on null-owner dimension parameters. The second byte of the version-5 trailer is the driven flag of the compact owned trailer, and it is set exactly on the parameters of dimensions whose frame stores style byte `1`. The reading of this item is that the second flag byte is the driven flag. No nine-byte or ten-byte trailer sets the second or the third flag byte, and no Design stream with these trailers holds a dimension frame with style byte `1`. In the nine-byte trailer the first four flag bytes are zero, and every parameter with a null owner reference sets its document flag. The null-owner dimension parameters of the ten-byte trailer clear it. Every frame with a set flag byte has the same remaining grammar as a frame with zero flag bytes. The decoder reads the second flag byte of a parameter that is not a document parameter as its driven flag and reports `parameter.driven-flag-assumed` for each stream with a decoded parameter that sets it. It keeps the other flag bytes unprojected and reports `parameter.trailer-flag-uninterpreted` for each stream with a decoded parameter that sets one of them.

**Need.** A writer of a driven dimension's parameter in either trailer must set the driven flag. The parameter of a dimension frame with style byte `1`, in a stream whose parameters use the nine-byte or ten-byte trailer, would settle its position.

**Conflict.** The comment expression trailer and the last five bytes of the wide expression trailer are the driven flag, a zero byte, the favorite flag, a zero byte, and the document flag. If the five flag bytes align with them, the first flag byte is the driven flag and the third is the favorite flag. Under that alignment the `TangencyWeight` feature inputs that set the first flag byte are driven, and the fourth flag byte, which null-owner dimension parameters set, is a zero byte.

### DR-1007. Header flag of a discriminated parameter record

**Question.** Does the header flag state that a sheet-metal rule holds the parameter, and what sets the byte at the same offset of a compact document parameter?

**Known.** `f3d.md` §3.1 "The form of an indexed parameter record follows the version of its type" places the flag in the discriminated form. A parameter that sets it is a `Thickness` parameter with the null owner, a ten-byte expression trailer of zero bytes, and an empty text value. A sheet-metal rule record, type `B42ED662-8999-4C1B-A744-759D1D446D55`, references each parameter that sets the flag and no parameter that clears it. A `Thickness` parameter that a timeline item owns clears the flag. The compact forms store a byte at the same offset, the eleventh of their fifteen zero bytes. It is `1` in each `Thickness` parameter that a rule record references. It is also `1` in some document parameters that an occurrence-source record of type `8ACBA2E5-5A4C-4438-8351-276DE96E6982` references, and `0` in other document parameters that such a record references. A frame that sets the byte closes the grammar of its form with the byte read as `0`.

**Need.** A discriminated parameter that sets the flag in a stream with no rule record that references it, or the stored property that separates the compact document parameters that set the byte from those that clear it.

**Note.** The decoder reads the flag in the discriminated form, keeps it unprojected, and reports `parameter.header-flag-uninterpreted` for each Design stream with a decoded parameter that sets it.

### DR-222. `MoveFace` and `OffsetFaces` member recipes that resolve no face

**Question.** Which face does a `MoveFace` or `OffsetFaces` bounded-face member select when the `DeleteFace` bounded-face rule resolves no unique face for its recipe?

**Known.** A member selects the face that its bounded-face recipe resolves in the input state (`f3d.md`, "The members of a `MoveFace` or `OffsetFaces` group also state the selection"). The rule resolves nothing for a member with a node whose structure matches no recipe-structure form, a positive program header that differs from the node count, or node ordinals beyond the prefix operands.

**Need.** The grammar of these node forms.

**Conflict.** None.

**Note.** Keep the group as the native selection.

### DR-223. `Shell` removed faces that the transition leaves open

**Question.** Which faces does a `Shell` removed-face group select when its bounded-face decoding is incomplete and the transition has cone, sphere, torus or spline faces, competing wall correspondences, or unequal or skin walls?

**Known.** A complete counted bounded-face program selects the unique input face with a boundary loop that matches every node. The recipe faces are the removed faces. For a group whose recipes do not resolve, the wall comparison selects the changed faces without a wall when their count equals the distinct member count and every changed face has a row to compare. Cone, sphere, torus and spline faces require other carrier comparisons. A kept face whose wall merges with another face and a removed face with an unrelated face at the wall offset can change the wall-derived membership while preserving its count.

**Need.** The remaining bounded-face node forms and wall comparisons for the other surface families.

**Conflict.** None.

**Note.** `feature.shell-removed-faces-assumed` names differing recipe and wall-derived face sets and retains the recipe faces. Keep a group native when both its recipes and wall comparison are unresolved. Unequal inside and outside offsets project as a neutral Shell with `BothSides` and absent total thickness. `feature.shell-independent-walls-native` records the retained source offsets and parameter expressions.

### DR-126. Auxiliary references of a Move scope

**Question.** How do the ordered references beyond the body group, its members, and the transform record affect the motion of a `Move` scope?

**Known.** `f3d.md` §3.1 defines the marked transform in the marker-zero, marker-one, and inline-typed prologues. The decoder applies the stored matrix to the group's whole bodies. `f3d.md` §3.1 identifies the translation-distance owners of a form-0 scope and the angle owner of a form-1 scope. The decoder identifies these owners among the scope's reference members by scope and local ordinal and classifies them when their values agree with the matrix. With no prefix or code words `0, 0`, each distance equals its translation component. With code words `0, 1`, the distance vector has the length of the translation. Other code words leave the owners unclassified. The angle owner agrees when its angle modulo a full turn equals the matrix rotation angle or the full turn minus that angle. It records `feature.move-scope-references-assumed` with the remaining auxiliary reference indices and names identified owners whose values disagree with the matrix. A form-2 transform also records `feature.move-transform-references-assumed` for its four construction references (DR-375).

**Need.** Controlled saves that change the rotation axis, the point-to-point source and target constructions, and other auxiliary selections independently while preserving the stored matrix.

**Conflict.** None.

**Note.** The auxiliary references are assumed to leave the stored motion unchanged.

### DR-306. Marked Move transform records outside the rigid-transform rules

**Question.** What do the leading scalar, nullable reference slots, unsigned prefix codes other than the form-0 `Move` code words `0, 1`, source forms four and six, and omitted-matrix marker of a rigid-transform record control, and what does a transform record that the marked reference of a `Move`, `Modify`, or `MoveFace` scope selects store when its class type is not the rigid-transform type, its frame is not a length of its type version, or its form word is not the form word of its frame length?

**Known.** `f3d.md` §3.1 "`Move` scopes with a fixed transform frame" gives the rigid-transform type, the form word of each frame length of type versions 2 and 3, and the marked reference that selects the transform record. A marked reference selects a 256-byte record with form word `5`, the zero run, and a rigid matrix at offset 48. The decoder keeps the matrix of each marked record with a finite leading scalar, six nullable reference slots, two unsigned code words, and source form `0`, `1`, `2`, `4`, `5`, or `6`. Zero leading scalar and first two null slots reduce this prefix to the four-slot form. `Move` and `Modify` scopes record `feature.move-transform-assumed` with each fixed-frame rule that the record breaks, and `MoveFace` scopes with each frame-length or form-word rule. A nonzero leading scalar, present leading references, partially populated remaining reference run, or nonzero code words other than the form-0 `Move` code words `0, 1` of `f3d.md` §3.1 project with `feature.move-transform-prefix-assumed`. Forms four and six project with `feature.move-transform-mode-assumed`. These fields are assumed to leave the stored matrix motion unchanged. The native operation retains the scalar, references, inline type GUIDs, code words, and matrix-omission flag. A one matrix-presence marker omits 128 matrix bytes and preserves the common tail. Form four, and form five with zero scalar, null references and zero codes, admit identity from a 77-, 78-, or 80-byte tail with `feature.move-transform-identity-assumed`; §3.1 gives the 128-byte version-3 omitted-matrix frame of the 80-byte tail.

**Need.** `Move` saves that set a nonzero leading scalar or a code word other than the form-0 `0, 1`, each recording the frame length and the bytes from offset 11 to the matrix.

**Conflict.** None.

**Note.** None.

### DR-400. Plane-frame record of a plane selection

**Question.** Which fields of the record that an entity-selection member's trailing reference names state the selected plane's frame, and does every plane selection store one? Which record binds the primary identity of a plane selection to its `WorkPlane` scope?

**Known.** The trailing reference of an entity-selection member that selects a plane names a record with type GUID `CC54ECAD-6DE8-4DE1-803D-BBE0E751975B`. The record stores the plane origin in centimetres and the unit u and v axes, each as three f64 values. A selection of the XY, XZ, or YZ origin plane stores the canonical origin-plane frame, and a `WorkPlane` selection stores the columns of the `WorkPlane` matrix. The decoder takes an origin plane's frame from the canonical table by ordinal and does not read or compare the record. It states the frame of a Draft origin plane, and of a Revolve origin-plane or `WorkPlane` target, only when the scope's Design stream binds one component. A Revolve target with no face-recipe member and one primary identity `p` that the Design type table rules out as an origin entity, and with exactly one `WorkPlane` scope at record `p + 1`, is read as that plane with `feature.revolve-plane-work-plane-assumed`.

**Need.** Saves with an angled `WorkPlane`, a `WorkPlane` moved after the selecting feature, and an origin plane of a non-root component, to type the remaining bytes of the record and its refresh rule.

**Conflict.** None.

**Note.** Draft and Revolve plane targets use the same selection member.

### DR-401. MoveFace matrix interpretation and unclassified references

**Question.** Which coordinate frame and composition order apply to the marked MoveFace matrix, how do unclassified references and prefix fields affect it, and which transform families occur when registration is unavailable?

**Known.** `f3d.md` §3.1 defines the marked references and counted table. The decoder prefers a unique frame registered as rigid-transform GUID `84E6245F-1CDC-4FB8-8048-163A85FAD3F0`, version `2`. With no version-2 frame, it takes a unique version-3 frame and prefers a frame that keeps the version-3 frame-length and form-word rules. A version-3 frame without the rigid-transform grammar reports `feature.move-face-type-conflict` when no other frame is admitted. It excludes positively conflicting registrations and reports `feature.move-face-type-conflict` when they leave the marked record without an admitted candidate. With no admitted registered frame, a unique framed candidate whose registration is unavailable keeps its motion with `feature.move-face-type-assumed`. Two admitted candidates within the preferred category remain ambiguous. Nonzero scope prefix fields keep marked motion with `feature.move-face-prologue-assumed`. Each projected matrix carries `feature.move-face-matrix-assumed`: the row-major matrix maps input face points to output model coordinates, with its translation converted from centimetres to millimetres. The complete ordered table, transform coefficients, prefix references and source prologue remain retained. References outside a bound group/member run and the transform remain unclassified; an unresolved selection leaves the entire table unclassified. Each unresolved table and each bound table with surplus references carries `feature.move-face-scope-references-assumed`. Form-two construction references retain the separate assumption in DR-375, and other transform-prefix fields retain DR-306.

**Need.** Controlled native saves that vary one pivot, construction point, selection, occurrence frame or move definition, joined through registered target identities, matrix coefficients and before/after geometry.

**Conflict.** Public move API semantics permit arbitrary translation and orientation and use centimetres for database lengths. These facts leave the raw matrix frame, composition order and reference roles unresolved.

**Note.** The most likely reading treats the matrix as evaluated face motion and the unclassified references as its construction history. Missing source and definition records preserve each decoded motion under the named assumptions. Historical face binding compares changed plane and cylinder rows under the complete projected motion, including small linear components retained by the transform law.

### DR-402. MoveFace motion with an unresolved face group

**Question.** Which face selection does a MoveFace scope name when its marked group cannot bind one complete ordered face-member run?

**Known.** The decoder retains the marked motion with a scope-native face selection when the group mark, role, identity, ordinal or member run is unresolved, or the transform is missing or repeated in the ordered table. It reports `feature.move-face-selection-unresolved`. Repeated table values name one transform record. A bound group retains its native identity for the historical face-recipe rule in DR-222. Matrix interpretation and unclassified-reference assumptions in DR-401 also apply when the selection is unresolved.

**Need.** A group grammar and membership proof for each unresolved native selection form.

**Conflict.** None.

**Note.** A table inconsistency leaves the independently decoded transform intact.

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

**Known.** The specification states the edges from the blend faces of the transition when every new carrier is a cylinder or torus blend. A sphere, spline, or variable blend has no spine row. A tangent chain rounds more edges than the group has members. A historical partial selection can name an edge that the transition keeps. The covering rule takes every other shared edge that the transition deletes as consumed by the blend. When a fillet between planes passes a parallel edge 1.5 mm from the rounded corner, the supports share both edges, the rounded edge covers the extent, and the consumed edge covers half of it.

**Need.** Controls that fillet one edge of a tangent chain, a corner of three rounded edges, and an edge between curved faces, controls that show whether a selected edge can keep its slot, and controls whose blend consumes a parallel edge between curved supports or along the whole blend.

**Conflict.** None.

**Note.** Keep the operand proofs for these groups.

### DR-653. `Chamfer` edges on bevel faces without a plane carrier

**Question.** Which edges does a `Chamfer` group select when its transition inserts a bevel face on a carrier without a plane row, such as the bevel of an edge between a plane and a cylinder?

**Known.** The specification states the edges from the bevel faces of the transition when every new carrier is a plane and both supports of each bevel are planes. A chamfer along a chain that crosses a cylinder inserts bevel faces whose carriers have no plane row. The history rows of a cone carrier state an axis line and no half-angle.

**Need.** The history rows of a controlled chamfer on the edge between a plane and a cylinder with two distances, and the row that states the half-angle or the apex of the bevel cone.

**Conflict.** None.

**Note.** Keep the operand proofs for these groups.

Bounded-face Chamfer members are tracked in DR-481.

### DR-390. Fillet set flag of the chordal and compact forms, marker bytes, and compact flag run

**Question.** Do the flag bytes of the chordal and compact fillet set forms encode tangent chain, do those forms store curvature continuity, and what do the set marker byte, the `00 01` tail of the current flag run, and the compact flag-run form encode?

**Known.** `f3d.md` §3.1 "A parameter-owned `Fillet` scope names its fillet sets in a prefix" gives their positions and admitted values. The decoder stores the flag of every set form as the set's `tangent_chain`, reads curvature continuity in the constant form only, and stores `rolling_ball_corner = true` for the compact flag run, whose first byte is `01`. The decoder admits the compact run `01 01`; a compact frame can store `01 00` in its place. Compact frames store zero bytes, 16-byte-aligned addresses, UTF-16 text fragments, or unrelated f32 values in the nine bytes after the compact run's `01 01` and in the four bytes at compact set offsets `+70` and `+89`. The decoder reads these bytes as padding and reports `feature.fillet-padding-assumed` when one of them is nonzero.

**Need.** Chordal and compact Fillet saves with tangent chain off, with curvature continuity, and with setback corners.

**Conflict.** None.

### DR-460. Chamfer set flag of the other forms, entry count, fixed byte, and auxiliary record

**Question.** Do the flag bytes of the two-distance, distance-angle, and compact chamfer set forms encode tangent chain, does the entry run count the selected edges or the edges after tangent propagation, and what do the zero byte that ends each entry, the fixed `u8 1`, the entry-run marker forms, and the auxiliary record encode?

**Known.** `f3d.md` §3.1 "A parameter-owned `Chamfer` scope names its chamfer sets in a prefix" gives their positions and admitted values. The decoder stores the flag of every set form as the set's `tangent_chain`, the marker as the list's `form`, the auxiliary record index as the set's `auxiliary_record_index`, and the entries per set as the set's `entry_count`. When the selected edges have no tangent neighbour, `E` equals the number of selected edges.

**Need.** Controlled saves of two-distance, distance-angle, and compact chamfers with tangent chain off, and a chamfer on one edge with tangent neighbours.

**Conflict.** None.

**Note.** Projection through explicit set references retains the flags and corner type in feature properties. A set with a true flag reports `feature.chamfer-tangent-chain-assumed`; a miter or blend corner reports `feature.chamfer-corner-treatment-unresolved`.

### DR-467. Face that a Hole's `bounded_face` operands name

**Question.** Which face do the `bounded_face` operands of a Hole scope name?

**Known.** `f3d.md` §3.1 "A `Hole` scope starts after its indexed header with eight zero bytes" calls these operands start-face operands and admits the Through All form by their recipe kinds and scope-reference order. In an Extents To Hole on a plate, their candidate lanes name the bottom face of the plate, and the hole starts on the top face.

**Need.** A Hole whose start face, end face, and body bottom face are three different faces.

**Conflict.** None.

### DR-254. Reference edges of a Hole's placement distances

**Question.** Which role-`0x0000002100000000` group does each of `RefDim1` and `RefDim2` measure from, and does a distance carry a sign?

**Known.** A Hole placed by reference-edge distances stores `RefDim1` and optionally `RefDim2`. One distance has one role-`0x0000002100000000` group, and two distances have two such groups. Its carrier point stores the resulting position.

**Need.** Controls with two different distances, with the edges selected in both orders, and with a hole on the other side of an edge.

**Conflict.** None.

**Note.** The Hole projects from its carrier point and reports `feature.hole-reference-dimensions-native`. Unproved distance-to-group associations retain the scalar dimensions and report `feature.hole-parameters-independent-assumed`.

### DR-470. Independent parameters of a partial Hole

**Question.** Which Hole parameters remain active when the extent controls, parameter associations, or entry-treatment dimensions leave the complete construction unresolved, and which standard sizing controls define a neutral clearance or thread specification?

**Known.** Extent control `1` can coexist with a `HoleDepth` owner and an absent saved-depth property. The parameter owners retain the diameter, tip, treatment and reference-distance expressions and evaluated values. The sizing block retains four ordered names for standard, fit, size and fastener, followed by the entry-type word for simple, counterbore or countersink form. The standard, size and fit names of a clearance hole fill its neutral clearance specification; that specification's modeled, cosmetic, hand and depth fields take fixed values and report `feature.hole-clearance-fields-assumed`.

**Need.** Controlled non-distance Holes that vary the stored depth independently of the target, partial and competing treatment parameter sets, and standard sizing controls that establish modeled, cosmetic, handedness and depth settings.

**Conflict.** None.

**Note.** The decoder retains independently admitted dimensions, parameter expressions and extent metadata on a neutral Hole. A readable entry-type word selects the active form; unmatched treatment parameters remain attached. An unresolved extent, entry-type or parameter association reports `feature.hole-parameters-independent-assumed`; an incomplete or conflicting shape reports `feature.hole-shape-partial-assumed`. The four sizing names remain source properties. A sizing block that gives no clearance specification reports `feature.hole-standard-sizing-assumed`.

### DR-471. Hole drilling direction with an unavailable control

**Question.** Which drilling direction applies when a Hole retains its carrier and entry points while its direction control is unavailable?

**Known.** The carrier stores a model-space normal, and its admitted point forms give the entry positions. Direction controls `0` and `1` orient the drill against and along that normal respectively. Legacy extent forms can retain these placements while omitting the direction control.

**Need.** Controlled legacy Holes with opposite drill directions at the same entry points and support plane.

**Conflict.** None.

**Note.** The decoder retains each entry position and carrier normal as an unoriented Hole axis and reports `feature.hole-carrier-axis-assumed`.

### DR-124. Partial-curve operands in profile selections

**Question.** What does the traversal flag of the entries of a group's first region that is the union of its further regions select, how do the piece ordinal and piece count of the entries of other typed profile loops and the two nonzero incidence words of a counted profile member select a partial curve, and which curve's direction sets the direction flag of the members of a coincident edge?

**Known.** The identity record stores region groups. Each region has counted boundary loops with exterior flags. Each curve entry stores a primary identity, a secondary identity, and three u32 operands. Counted profile members store a primary curve identity, a direction flag, and two nonzero incidence words. `f3d.md` §3.1 "**Side-face walk.**" states the traversal flag of the entries of a one-loop profile of whole lines in a stream with serializer magic `1234`. A complete neutral loop match, a unique atomic arrangement boundary program, or unique directed boundary rings resolve the selected region. The decoder builds the member-ring arrangement from lines, circles, circular arcs, bounded ellipses, and nonperiodic NURBS. A bounded ellipse leaves the arrangement undefined, because its edges have no certified pieces. The nodes are the end points of these curves and the intersection points of every two lines, circular arcs, or circles that are not collinear line segments. A node splits a line, circular arc, or circle that passes through it; a NURBS splits only at its end points. Edges leave a node in the counterclockwise order of their departure tangents, each turned by half its exact curvature over the linear tolerance. The tangents are exact for lines, circular arcs, and clamped NURBS ends with positive weights. Equal tangents order by signed curvature. A clamped NURBS end has only the sign of its curvature, so two equal signs leave the arrangement undefined, as do two equal exact curvatures. Another edge leaves along the first chord of its polyline, and a node with such a departure and more than two edges leaves the arrangement undefined. The selection stays native with `feature.profile-selection-native` when two edges meet away from their common nodes, as where a NURBS crosses another curve, or when one curve runs twice along one edge or in both directions along one fragment. The exact lines and circular arcs and the certified NURBS tubes of a face boundary, each in the direction of its walk, decide whether a point lies inside the face. A face holds a component when it holds the first node of the component that lies farther than the linear tolerance from every boundary piece. An island lies in each face that holds its component, and the innermost of these faces holds it; one face lies inside another when the other face holds its component. When no node of the component clears the pieces of a face, or no face that holds the island is the innermost, that face and the faces that hold the island have unknown islands. When the island has no unique outer walk, the innermost face and the island's own faces have unknown islands. A selection of a face with unknown islands stays native. A projected point within the linear tolerance of a NURBS tube of a face boundary takes the side of the face polyline, and the selection reports `feature.profile-polyline-side-assumed`. For member lists, projected points, curve programs, and self-crossing loops in the arrangement of every non-construction curve, a non-construction full ellipse or periodic NURBS without a negative weight has a box that holds it: the exact axis-aligned box of the ellipse, or the box of the control points of the NURBS. The box of a face holds its lines and circular arcs exactly and its NURBS edges within their certified tube bounds. The curve lies outside a face when its box misses the face box by more than the linear tolerance, or when its box, widened by the linear tolerance, meets and holds no boundary piece of the face and the box centre lies outside the face. When the widened box meets and holds no boundary piece of any face, the curve lies in the region of the smallest face that holds the box centre, and that face then bounds no arrangement region. For every other face that holds or can hold such a curve, and for every face and every other non-construction curve outside these families, the decoder takes the face without an island of that curve and reports `feature.profile-island-absence-assumed`. A selection whose profile takes no sketch regions reports neither loss. For a walk with an edge of coincident curves, the decoder accepts the edge when one distinct member that names one of its curves has that curve's traversal direction, and it checks no flag of the edge's other members. This filter can choose one of several walks with the list's curve identities and multiplicities, and it can reject the only such walk. The decoder reports `feature.profile-coincident-direction-assumed` for each bound selection in which a walk with such an edge has a list's curve identities and multiplicities. The member-ring search charges the arrangement work budget, and an exhausted budget leaves the selection native with `feature.profile-arrangement-capped`. The decoder matches a boundary program against the arrangement of the curves of the Sketch's profile loops, or of every non-construction curve when the Sketch has no profile loop. A typed region whose loops match no curve program is read through the same directed rings: each stored loop is a ring, its exterior flag makes it the outer ring or a hole, and the first operand of each entry is read as its traversal flag, with `1` against the curve's direction. Such a selection reports `feature.profile-loop-flags-assumed`. That reading does not read the other two operands. A counted selection of eight members in a Sketch that has zero neutral profiles and contains partially overlapping lines can carry incidence values `(2, 3)` on its first member. A counted selection of six members in a Sketch with two complete neutral profiles can carry `(2, 3)` on its second and fifth members, which are short lines, and `(3, 5)` on its third and sixth members, which are long lines. Each short line runs opposite the coincident part of the long line that follows it in the list, and both members have direction flag one; the two pairs lie on opposite boundaries. The decoder retains these feature selections. `f3d.md` §3.1 states the piece ordinal and piece count of the entries of a group's first region in the arrangement of that region's own curves, and the decoder enforces them for a first region that is the union of further regions. The decoder does not read the traversal flag of those entries.

**Need.** Controlled selections of adjacent regions with one shared curve, and of intersecting curves with distinct selected regions, to bind each operand to curve orientation and trim bounds. Selections with the same curve-identity multiset must vary the selected portions independently. Controlled selections of one region whose boundary edge is two coincident curves drawn in opposite directions, with each curve created first in turn, bind the direction flag of a coincident edge.

**Conflict.** None.

**Note.** A complete neutral-loop match requires distinct identities. Revolve profiles resolve through stored loops or directed member rings. An ambiguous match or a match that requires an interpretation assumption retains the native selection with `feature.profile-selection-native`.

### DR-125. Counted face-target recipe without a unique historical face

**Question.** Which counted bounded-face recipe clauses select a termination face when persistent references retain several preceding support faces?

**Known.** A termination group identifies the face operand. The operand retains its counted recipe nodes, ordered prefix references, and state-local support contexts. A unique preceding face or convergent support context supplies a historical selection.

**Need.** Controlled face-target selections that vary the selected planar face while the body topology and other prefix references remain fixed.

**Conflict.** None.

**Note.** Candidate membership, planar support, and absence of changed faces do not by themselves identify a selected face.

A compact Sketch-point termination of an Extrude is tracked in DR-480.

### DR-122. Derived Base Feature source and frame

**Question.** Which field of a `Derived Base Feature` scope outside the source-pair form identifies the derived source body, and which field stores the scope's local frame or mirror plane?

**Known.** The source-pair form stores source and result body entity references that join the Design body map in the same stream. The 711-byte frame stores nine typed-reference elements, a zero GUID, the kind, the ordinal, and an f64 of unknown meaning. The second body element of the 711-byte frame stores the Design body entity suffix of the result body. Every reference in the scope is a Design record of the same stream. The frame stores no matrix. The `DerivedBaseFeature` forms (707 and 491 bytes) store the same elements and roles, a part with a mirror label places its occurrence with an identity rotation, and no field in either form stores a plane or a matrix. The 761-byte frame has one additional typed-reference element with a role GUID that no 711-byte frame stores. Its record index is lower than the scope record index.

**Need.** A small design with two derived scopes and two source documents. Each source document must contain two geometrically equal bodies. Save one pair that changes one scope's source document. Save another pair that selects the other source body. Reverse the dependency creation order in a third pair. Keep result body identities and geometry fixed in these pairs. Save separate translation, rotation, and mirror changes. Keep the source body identity and local geometry fixed for these frame changes. Include a source component with one Sketch and zero bodies, then add one body while keeping its component identity. Include the deriving archive and both source documents at each save. Trace each scope through its local references and GUID fields to one DERIVED row, source body, and stored frame. Compare the f64 and the additional 761-byte element across these controls.

**Conflict.** None.

**Note.** The decoder projects the derived base scopes that name one owner as one feature at the owner's timeline position when the owner is a timeline item that no scope occupies and no other feature claims, no scope of the group holds its own timeline position, the scopes' history states chain, and no derived base scope without an owner reference continues or precedes the chain. The feature lists the group's record indices in its `derived_base_scopes` source property. A one-body source-pair scope that takes its owner's timeline position and has a captured result body projects as a Base Feature over that body and reports `feature.stored-bodies-recipe-absent`. Its local frame and mirror plane remain unresolved. Unresolved derived base operations remain native or retain a stored result snapshot with `feature.derived-base-unresolved`. Component context bindings identify the result owners of such scopes. The source-body and source-frame joins require the derivation references independently of those owner bindings.

### DR-552. Sphere frame assumptions and unresolved placement references

**Question.** Which typed prefix applies when its leading padding byte is absent, how do unresolved local-frame markers and placement flags change a sphere centre, and which support frame applies when its reference is unresolved?

**Known.** Reference-placed spheres retain a diameter owner, result operation, and placement reference. Placement constructions carry a solved matrix or an identity marker. Sphere axis rotation preserves its shape.

**Need.** Controlled saves with one-byte-short typed prefixes, nonzero placement flags and a changed sphere centre, and a reference-placed sphere with a resolvable external placement.

**Conflict.** None.

**Note.** The decoder reads a one-byte-short typed prefix with `feature.sphere-prefix-framing-assumed`. It uses the stored solved frame with `feature.sphere-placement-flags-assumed` when its flags are unresolved. It applies an identity support frame with `feature.sphere-placement-frame-assumed` when the referenced frame has no unique usable local definition. It preserves a rigid local matrix with `feature.sphere-local-matrix-framing-assumed` when its marker is unresolved, and applies identity with `feature.sphere-local-frame-assumed` when the local pose is unusable.

### DR-551. Sphere diameter references with incomplete ownership

**Question.** Which diameter applies when the referenced scalar has incomplete scope ownership or parameter-source binding?

**Known.** The sphere prefix identifies a diameter-owner record. Its evaluated scalar supplies the diameter in centimetres, and the owner names its scope and parameter source.

**Need.** Controlled saves that distinguish a stale evaluated diameter from a live value after an ownership or source-reference failure.

**Conflict.** None.

**Note.** The decoder uses a unique positive referenced scalar with `feature.sphere-diameter-owner-assumed` when ownership is unresolved, and retains the evaluated diameter with `feature.sphere-diameter-source-assumed` when its parameter source has no unique local definition.

### DR-123. BoxPrimitive support frame and planar offset composition

**Question.** How does a nonzero `OffsetDistanceX` or `OffsetDistanceY` owner combine with the stored frame in the typed-reference and marked-reference `BoxPrimitive` forms, and how do inferred-axis and flip fields change the support placement?

**Known.** Both forms store the result operation, a rigid box-local frame, five parameter owners, and a reference to the support placement construction. The support placement composes on the left of the box-local frame. The 1295-byte form stores one further typed reference at offset 517 outside its table.

**Need.** A scope in each form with a nonzero offset owner and a changed placement. Controlled support placements with inferred-axis or flip fields set.

**Conflict.** None.

**Note.** The decoder applies nonzero planar offsets along the box-local frame's x and y axes and charges `feature.box-offset-frame-assumed`. A stored rigid support frame with unresolved placement flags transfers with `feature.box-support-flags-assumed`. An absent, ambiguous, or unusable support placement uses an identity support frame with `feature.box-support-frame-assumed`. Dimensions and Boolean operation transfer with these assumed placements.

### DR-147. Region extent of a tracking-path Extrude selection

**Question.** Which Sketch region does an Extrude selection select when its member follows a tracking path?

**Known.** The path stores the primary curve, a one-based fragment ordinal, the fragment count, and the two curves bounding the fragment. The carrier lane is `2` or `3`. The selected fragment can border two bounded Sketch faces.

**Need.** Controlled selections of one region, of two adjacent regions, and of a region with an enclosed island, each in a Sketch whose curves cross, to bind the path to the selected side and to the filled island. Selections of each region of two crossing circles, and of the union of two of these regions, give such controls.

**Conflict.** None.

**Note.** A path selection retains its native selection and loss. The selected region can be the union of several atomic arrangement faces, which no single face proves.

### DR-146. Later ordinal in a fixed or leading-flag identity occurrence

**Question.** What placement does a fixed untyped or leading-flag occurrence with identity flag `1` and ordinal greater than one select?

**Known.** A standard 229-byte frame can store ordinal `2` with identity flag `1`. Attributed identity frames retain later ordinals. The leading-flag frame stores the same identity flag and one-based ordinal.

**Need.** Independent files that vary the occurrence ordinal and placement separately.

**Conflict.** None.

**Note.** Emit `assembly.local-occurrence-identity-ordinal-unresolved` and keep the occurrence out of neutral product structure.

### DR-142. Thread construction option bytes

**Question.** Which controls state side outside the extended prefix and hand outside the extended standard form? Which table identity and revision, with the thread type, designation, and class, determine the exact groove section, including crest and root truncation and radii? What does the second byte of the Thread construction marker select? In the forms other than the extended standard form, what do the first marker byte and the zero bytes before the trailer `01` select? In a form without the extended prefix, which field separates a cosmetic thread from a modeled thread whose scope has a null current history-state identity?

**Known.** `f3d.md` §3.1 "The three LP-UTF16 fields are the designation, nominal-size text, and profile name" defines the thread type string and four physical scalars. The extended prefix also carries class and side; its extended standard trailer carries hand. The native construction retains these values and the flank angle. The construction marker first byte is `0` or `1` and its second byte is `1` or `2`. In the extended standard form, the first marker byte is the full-length flag. Its second marker byte is `1` or `2`: threads on one external face store `1`, threads on two external faces and a thread on one internal face store `2`, and threads edited after other features store either value on one face. Thread type, designation, class, length, offset, hand, and the modeled option leave it unchanged. The other admitted forms fix each combination they accept. In the standard-prefix compact-marker form, the run before the trailer `01` is two or three zero bytes. Every byte in that run is zero in each Thread scope that carries it. A form without the extended prefix stores modeled flag `0` for both a cosmetic thread and a modeled thread whose scope has a null current history-state identity; in a scope with a null current history-state identity the decoder reads `0` as a cosmetic thread and reports `feature.thread-modeled-flag-assumed`.

**Need.** Raw final ASM face and cylinder orientation records for a modeled Thread with side flag `0` and class `6H`, compared with its selected historical face and its thread side flag. External and internal compact and compact owned Thread counterparts that separate the prefix zero lanes from the side control. Thread table identities and revisions tied to measured groove sections for each type, designation, class, and side. Final helical faces with opposite hands and final cylindrical carriers with opposite material sides. Thread scopes of the direct, owner-marked, and extended compact forms saved with one dialog option changed at a time. A cosmetic thread and a modeled thread of one form without the extended prefix, each saved with a null current history-state identity. For the second marker byte of the extended standard form: one external and one internal thread on faces made by the same feature, each saved before and after an edit of its face selection.

**Conflict.** A modeled Thread can store side flag `0` and class `6H` while its decoded final minor cylinder has radius `2.117` mm and a forward declared normal. The measurement of that face has an undetermined loop normal because its procedural boundary curves lack a derivative evaluator. The declared normal and the internal-side reading require reconciliation.

**Note.** `feature.thread-detail-native` records native retention of side controls, hand outside the extended standard form, flank angle, and the exact groove profile. A scope whose marker combination no admitted form accepts stays an unresolved Thread operation.

### DR-152. Mirror selection run with role 0x10

**Question.** What do the members of a Mirror scope's role-`0x10` group select?

**Known.** `f3d.md` §3.1 "A Design Mirror scope" lists the seed and plane groups. A Mirror scope can also store a counted role-`0x10` group whose members are class-256 persistent selection records. Such a scope stores a role-`0x5` plane group with one bounded-face recipe member. When a Mirror scope has no non-empty role-`0x4` or role-`0x8` group, the decoder uses the role-`0x10` group at scope-reference ordinal one as the seed group of the mirror construction. The members of that group supply face seeds when each decodes as a bounded-face operand. A plane entity selection can have a 29-byte compact identity record whose primary identity is not an origin entity. When the type table does not list that identity under the construction-plane type, the scope has no plane reference, and the face candidates of the primary and persistent identities give no plane, the decoder reads the plane from the scope in the same stream at the compact identity plus one. That scope must be a WorkPlane with a finite frame. Each candidate face must be absent from the preceding history state, or lie on one surface with an analytic non-plane definition in that state. A candidate face that gives a plane, a candidate face that does not meet these conditions, or a history that holds the identity and supplies no candidate face voids this reading. The decoder reports `feature.mirror-plane-work-plane-assumed` with each candidate face and its defect.

**Need.** Mirror features with controlled changes to the selected objects, to find which seed kind the group carries and how it relates to the seed group. A compact plane selection whose identity the type table does not list under the construction-plane type, to show what that identity names.

**Conflict.** None.

**Note.** None.

### DR-153. Instance discriminator of a component path

**Question.** How does the u32 instance discriminator of a local component path select one expanded instance when several instances of the target have the listed containing carriers?

**Known.** No two component-path records of a Design stream share a record and a discriminator. A path that lists the full chain of containing carriers names one instance. The initial element list can name several instances below its listed carriers. The first closing reference run carries the full ordered occurrence chain defined in `f3d.md` §1.4. An initial element list can also reach no instance. A target with 16 expanded instances and 16 component-path records can store target discriminators 0 to 15 once each, and the order of the target discriminators equals the order of the Component element discriminators. No tested order of the expanded instances reproduces the target discriminators: ordering by the records from the outermost carrier inward matches for some targets and differs for others.

**Need.** Designs with a repeated holder of known instance order, made by copy and by pattern, with the component-path records before and after reordering the instances.

**Conflict.** None.

**Note.** A path whose initial elements and local closing occurrence chain name one instance supplies its stored world pose. A path that still names more than one instance is placed only when the layout-consistency inference in `docs/format-support.md` selects exactly one assignment; it is then reported as `assembly.local-occurrence-path-pose-inferred`. Otherwise it is reported as `assembly.local-occurrence-path-pose-unresolved`.

### DR-150. History namespace of copied feature scopes

**Question.** Which native relation binds a parameter scope to its ASM history when several histories contain the same endpoint pair and the scope has no unique body-map or whole-body recipe binding?

**Known.** Current and preceding state identities are local to one ASM history. Independent history graphs can contain the same endpoint pair and complete predecessor chain. A result body-map carrier, whole-body recipe candidate faces, or agreement of all qualified face candidates from the scope's own edge recipes can bind a scope to one BREP basename. Copied feature groups can retain the same endpoint pairs in separate histories. Each component's bodies live in their own BREP blob, and a history's id names its blob. The decoder first uses the scope-history binding map. For a scope without a map entry, it uses the unique history with the state pair, with direct transitions before intervals. When the preceding state is unavailable, it uses the unique history with the result state. If the output interval remains unbound, the decoder reads the feature's history properties and output transition interval from the one history of the BREP blob that holds every body the feature outputs, or that a Base Feature resolves, when exactly one history of that blob holds the pair, and reports `history.output-blob-history-inferred`.

**Need.** The native reference chain from each copied scope to its history or BREP basename. Files that change the copied scope's source component while they retain its endpoint pair.

**Conflict.** None.

**Note.** An ambiguous interval retains its native scope, ordered references, state identities, and candidate histories with `feature.output-state-chain-unbound`.

### DR-1008. Generated-body selections with an unresolved history

**Question.** Which native relation selects the history of a body-pattern seed or `RemoveBody` operand when scope-history bindings and state identities leave several candidate histories?

**Known.** Generated-body selections use the scope-history map and state tests in DR-150. When every selected slot is absent from the live bodies and the selecting history stays unresolved, the decoder takes, for each slot, the one state of any history that inserts it and the one same-stream scope with that state as the producer, and reports `feature.generated-body-history-unresolved` with the selecting feature, the body slots and that assumption. Without such a state and scope, it retains the Historical pattern seed or native `RemoveBody` group with the same loss.

**Need.** Controlled designs with repeated endpoint pairs and one omitted seed body, with the native reference chain from the selecting scope to its history.

**Conflict.** None.

**Note.** The producer rule is in `f3d.md` §3.1.

### DR-151. Result ownership of copied feature scopes

**Question.** How does a copied feature scope relate its construction component to the product definition of its result bodies?

**Known.** A scope has a component owner. Its ASM transition interval identifies affected body slots in one BREP namespace. An affected active body can belong to a different product definition from the scope's component owner.

**Need.** A two-component design with a copied feature whose history interval touches bodies owned by both components. Use equal geometry on the two bodies. Save one pair that changes the copied feature's component and preserves its construction references, body identities, local body geometry, and component placements. Save separate pairs that change one body's component while the feature component and construction references stay fixed. Start with identity component placements. Then translate and rotate one component in separate saves while local body geometry and ownership stay fixed. Trace the scope owner, active body-map pairs, and local frame of each result body through each pair.

**Conflict.** None.

**Note.** Result body links outside the scope's product definition remain native with `scope.output-owner-mismatch`. A body with changes after its last linked feature receives a native operation and `feature.body-history-incomplete`. A last feature without a readable history state, a history without a state order, and a body whose slot is unread receive them too, and the note names an unread state. A body that no single decoded history holds receives no interval; it receives `feature.body-history-unread`, and the note states that changes after its last feature are not checked. When exactly one product definition lists the body, that definition owns the operation; the note names it. Zero or several such definitions retain the unowned operation and imported final shape with `feature.body-history-owner-unresolved`; its note states no owner or several owners. A tail made of geometry-unchanged transitions preserves the linked history. An interval to the bound head with unresolved body incidence or transition links retains the native operation.

### DR-162. Component paths with two nested occurrence placements

**Question.** What pose does a component-path record state when its path holds a root component, a cross-document occurrence placement of the same role, this document's occurrence placement, and a second cross-document occurrence placement of the external occurrence's role?

**Known.** Such records store a rigid matrix after the path, like the records `f3d.md` §1.4 defines. The local element can state the occurrence-placement type inline. The decoder reads none of these records.

**Need.** Records of a design that nests a design two levels deep with one occurrence moved at each level, to find whether the matrix is the world pose of the innermost occurrence or of the middle one.

**Conflict.** None.

**Note.** None.

### DR-164. Primary edge-recipe references with several face fragments

**Question.** Which selector rule selects the edge when the primary-reference lane unions, distinct-support-face constraint, and recipe-boundary constraint leave zero or several edges? Which face does a lane select when its persistent names bind to disjoint face sets? Does the singleton deletion proof apply to a one-side Chamfer recipe?

**Known.** The first side carries one-based prefix-reference ordinals. A prefix reference can bind to several preceding faces through persistent token and Design-reference tags. Each first-side reference lane contributes the union of its live face-fragment loop edges. The lane intersection requires distinct supporting faces. The recipe-boundary constraint selects a unique edge. A standard two-side Chamfer or Fillet recipe has transition proof for a singleton lane intersection when the owning transition deletes that edge and the persistent names within each lane have a common face. Chamfer and Fillet operands, including fragment-lane operands, can make singleton selections outside the candidate-face boundaries. In each such selection the owning transition deletes the selected edge and changes its supporting faces. Stored recipe programs, face-loop boundaries, edge revisions, and state transitions prove the same reading in independent designs. A one-side Chamfer operand has a 37-word program. Its side header is `0`, its scalars are `[1, 2, 1]`, and its two topology entries have boundary counts `2` and `7`. Its first side names two faces whose common edge a later history state deletes, and that state changes both faces. The recipe candidate face is a third face. This one-side reading retains `history.edge-selection-inferred`. A disjoint-name lane still uses a deleted-edge restriction as an assumption.

A Fillet group of the same form can have two paired members that each retain the same two edges, and the transition deletes both edges.

The same two-edge ambiguity can remain after the candidate-boundary, deletion, and radius constraints: two operands of one scope each keep a different pair of remaining edges. Such standard two-side recipes have compact payload prefixes, zero topology entries, and four populated preceding face lanes. Each operand's first side includes a face-fragment lane.

**Need.** The selector rule for several remaining edges and for first-side lanes with disjoint edge sets, the face-binding rule for disjoint persistent names, and an independent one-side Chamfer design that distinguishes the selected edge from collateral deletion. Controlled recipe changes that preserve the prefix references and vary the selected fragment must distinguish the candidate rules. For paired members with the same two remaining edges, identify the fragment selector. Confirm the roles of the third and fourth prefix lanes in independent designs that distinguish selected-edge identity from neighboring-face context.

**Conflict.** None.

**Note.** An unresolved member retains the native group. A singleton without the deletion proof and a disjoint-name reading report `history.edge-selection-inferred`. A non-unique or incomplete reference proof reports `history.edge-midpoints-unresolved` when the feature requires edge samples. A Chamfer operand can have two common preceding edges of which the transition deletes one, or deletes both. A Fillet operand can name three faces whose edge-set intersection in the preceding state is empty. These cases require the recipe selector proof. A deletion filter changes the cardinality of an ambiguous intersection.

### DR-1060. Chamfer members whose first side names three face references

**Question.** Which edge does a standard Chamfer recipe select when its first side carries three non-null face references, and how does a group with several members that keep the same two candidate edges select its edge set?

**Known.** A member whose first side names two face references selects an edge that both reference lanes hold among the changed edges of the owning transition. The edges that the transition deletes include every selected edge. A member whose first side names three references has an empty intersection of the three lanes' changed-edge sets. Such a member can occur in a group together with members that resolve by two references; the group then keeps its resolved edges as a partial selection. Members that keep the same two candidate edges are interchangeable, and the pair of edges they cover is the same under both assignments. A recipe's selector contexts can exclude the edge that the first-side lane intersection proves, so they do not corroborate the lane intersection for these members.

**Need.** A controlled Chamfer whose selected edges bound faces that persistent names bind to several fragments, with edge sets chosen so that exactly one assignment of three-reference members to edges exists. The role of the third first-side reference in the selector.

**Conflict.** None.

**Note.** The group retains its resolved edges and the unresolved member identities as a partial selection, and a feature that needs edge samples reports `history.edge-midpoints-unresolved`.

### DR-170. Unresolved DeleteFace operand bindings

**Question.** When the transition deletes more faces than the group has members, which deleted faces does the group select? Which native rules bind DeleteFace operands with an empty live prefix-face lane, repeated edge-incidence matches, or a prefix-fragment face fan that fails the live topology match?

**Known.** `f3d.md` §3.1 "The scope's preceding and current state identities delimit" settles a group whose member count equals the deleted-face count. Healing can delete a further face, so a larger deleted set does not separate selected faces from healed faces. Such a group resolves only through the complete-boundary rule in `f3d.md` §3.1. The transition of a one-member group can delete four faces. The member's recipe then binds one face through an endpoint fan at a vertex where one edge has four face uses (`f3d.md` §3.1), and the transition's three other deletions are healing. Some operands have a referenced prefix ordinal with an empty face lane in the preceding topology. Some have multiple boundary-edge matches after incident-pair and cyclic endpoint-fan matching. Some have prefix lanes that repeat a face fragment while the boundary endpoint fans require another face.

**Need.** The role of a segment that opens with `[-1, -1, 1]`; the binding of an empty prefix lane to a preceding face; the directional or topology-entry rule that distinguishes repeated edge matches; and the ordinal and fragment rule for the mismatched prefix fan. Independent selections that vary these controls must distinguish the possible bindings.

**Note.** A member stays native when no complete face boundary in the preceding topology binds its recipe uniquely: a node has no parsed clause structure, a node's root or other-face lane holds no face with a complete boundary, a node matches several edges of one boundary, an endpoint clause matches no fan or corner walk at its vertex, or two faces bind every node. The group then retains its native identity and reports `feature.face-selection-native`.

### DR-262. Faces of transition-inferred Extrude face profiles

**Question.** Which preceding faces does an Extrude face profile with unresolved members select, and which of them does each unresolved member select?

**Known.** The unresolved members have a node with no parsed clause structure, a node that binds no unique boundary edge of one candidate face, or positive prefix references whose face lane is empty in the preceding topology (DR-170). The decoder reads an Extrude whose native profile is the scope's one profile-role construction group in the scope's stream, whose members each have no historical face slot or one and no active face, whose resolved slots are distinct, and which has at least one unresolved member. It infers the faces from the scope's direct transition when the Extrude is one-sided and blind along the profile normal or its reverse, starts on the profile plane, and has no draft. A preceding or inserted face with several surface bindings, a surface with several plane rows, or no surface binding, and an inserted face on a surface without geometry rows, keep the root native. A preceding face on a surface without geometry rows lies on each plane that holds all its vertices, or on every plane when its loops cannot be read, and counts there as a face that cannot be read. The inserted planar faces parallel to a plane at the blind distance from it are taken as end caps that repeat the swept profile. A preceding face can be the profile when it is a resolved face, when the transition deletes or updates it, or when the Extrude makes a new body. On each such plane and side whose preceding faces can be the profile, the preceding planar faces whose interior sample points lie inside the caps are the root's faces when they can be the profile, number one per root member, include every resolved face, cover the caps' area, and have a union outline that lies on the cap boundaries and passes through every cap vertex. Each cap and selected face edge is a line on a straight carrier or a circle where the face meets a cylinder about an axis parallel to the plane normal. A loop has no readable outline when a vertex has several point bindings or positions, a coedge several rows, or an edge several vertex rows, curve bindings, axis rows or samples, or when a neighbouring loop of a circle edge has no single owner face or that face no single surface binding. The root stays native when an inserted planar face has no unit normal, when a selection that can be the profile fails these conditions, when two such selections differ, when a cap over a plane with such a face cannot be read, or when a face whose edges or sample point cannot be read reaches into a cap and either that face or the selection on its plane can be the profile. Such a face is drawn by its outline points, or by its vertices when its edges cannot be read. It reaches into a cap when its loops cannot be read, when a point is not finite, when none of its loops has three points, when one of its points lies inside the cap away from the cap boundary, when a cap vertex lies inside the face away from its loops, or when the mean of its largest loop's points or a point of a grid that divides its bounds into 24 parts on each axis lies inside both the face and the cap, away from their boundaries. This reading reports `feature.extrude-profile-cap-faces-inferred` with the faces at the blind length outside the caps, the unreadable faces clear of the caps, and the number of planes and sides whose faces no member resolves and the transition neither deletes nor updates. When no cap selects such a face set and at least one member is resolved, the transition's deleted faces are the root's faces when they are as many unique preceding planar faces as root members, include every resolved face, and lie on the plane of the resolved faces. A Cut can also delete coplanar faces outside the selection; this reading reports `feature.extrude-profile-deleted-faces-inferred` with the coplanar faces that the transition updates and keeps and the candidates that the end-cap search passes over. When both readings give a face set and the sets differ, the root stays native. Each resolved member keeps its face at its member position, and the remaining faces take the unresolved positions in increasing stable face-slot order; a profile with two or more unresolved members reports `feature.extrude-profile-face-order-assumed`.

**Need.** The node grammar and the empty-lane binding of DR-170, so that each member's recipe selects its face.

**Conflict.** None.

**Note.** On a plane whose preceding faces can be the profile, a search past a local cap of 256 faces or 256 caps on the plane, 4096 outline points, or 4096 edges on one face keeps the root native and reports `feature.extrude-profile-face-search-capped`. A root that stays native for a contradicting or unreadable face set, or for inferred faces that fill no member order or historical selection, reports `feature.extrude-profile-face-set-disputed` with the reason.

### DR-304. Fallback faces of Extrude bounded-face operands

**Question.** Which faces does an `Extrude` bounded-face operand select when its recipe nodes have no unique topology binding?

**Known.** The nodes of a bounded-face recipe name, by prefix-operand ordinal, faces around the boundary of the recipe's face. Without a unique binding, the decoder selects faces with the candidate and support rules of `f3d.md` §3.1 that follow the alternate-selector proof, and tests the selection unless the grouped Design-reference intersection holds a face or the operand has alternate-selector candidates. The test takes the prefix operands that the nodes with a parsed clause structure name, by root, first prelude value and fan values, or every prefix operand when no node has a parsed structure, as naming faces around the selected face's boundary; a node without a parsed structure, or one that names an ordinal outside the operands, names none. Such an operand supplies the preceding-topology faces that its references with a nonnegative Design reference name; it contradicts nothing when a negative reference of it names a face, when another reference names no face, when a named face lies outside the preceding topology, or when a named face has no rebuilt boundary. The selection fails when every selected face has a rebuilt boundary and the selection shares no boundary vertex with the faces of one operand. A failing selection is withdrawn. The legacy start and termination candidate rule then gives the operand only a face that is not withdrawn and shares a boundary vertex with every such operand. The profile-group carrier rule pairs each member with the face of the same position in increasing stable face-slot order; it takes no face set that holds a face withdrawn from one of its members, or that pairs a withdrawn member with a face that does not share a boundary vertex with every such operand of that member. Each refused face or face set reports `feature.face-operand-fallback-contradicted`. The sketch-plane start and termination rules of the same scope can still select any candidate face, the withdrawn one included.

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

**Known.** `f3d.md` §3.1 reads the operation at offset 21 with values one through three and keep-tools at offset 25 in the class-387 form, and reads keep-tools at offset 30 and the operation at offset 31 in the class-329 form. The operand-list form stores a zero-based operation at offset 21, keep-tools at offset 29, and the operation-entry count at offset 31.

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

**Question.** Which of the three lists between the tool list and the target list does an operand-list `Combine` prologue omit when it stores two?

**Known.** `f3d.md` §3.1 gives the three middle lists: an empty list, the created-body list, and the consumed-body list. Saves of every operation, keep-tools state, new-component state, result-lump count from one to three, and tools from one or two components store three lists. The two-list form can carry two empty lists with a companion, or an empty list followed by consumed-body references without a companion.

**Need.** A save that stores the two-list form with a nonempty list, together with the result bodies and the consumed tools of that `Combine`.

**Conflict.** None.

**Note.** A two-list form without a companion reads the middle lists as the created-body and consumed-body lists and reports `feature.combine-middle-list-omitted-assumed`. DR-561 tracks the destination of a new-component request.

### DR-272. Prologue of a Combine frame whose byte 20 is one

**Question.** Which prologue does a `Combine` frame store when byte 20 is one, its fixed fields match both the standard and the operand-list prologue, and its lists do not complete the operand-list prologue?

**Known.** `f3d.md` §3.1 gives the standard and operand-list prologues. A standard join and an operand-list prologue with an empty property block both store `01 00 00 00 00` at offsets 20 through 24. An operand-list prologue without a companion can also store zero at offsets 26 through 32. In this overlap the standard reading gives a join whose keep-tools Boolean is the operand-list operation byte, so an operand-list cut reads as a join that keeps its tools. A complete list reading checks the ordered operation identities, the list rules, the counted GUID and the exact reference-table boundary, and §3.1 selects the operand-list prologue for a frame without an expanded named tail that it reads. A `Combine` cut with Keep Tools off whose tool splits the target into two bodies can store a frame in the overlap, and the operand-list reading gives that cut. A frame in the overlap whose lists do not complete the operand-list prologue is read in the standard prologue and reports `feature.combine-prologue-assumed`. DR-560 covers the expanded named tail. A frame whose lists match its operands but contradict other required list fields reports `feature.combine-operand-list-refused` when no prologue reads it.

**Need.** A frame of this kind with its operation and keep-tools state shown by result volumes or by a Fusion readback.

**Conflict.** None.

**Note.** For a non-operand-list Combine stored as Join with kept tools and no deleted bodies, the history binding assumes Cut and reports `feature.combine-kept-tools-cut-assumed`. The native operation remains stored in the scope.

### DR-921. Tools of an operand-list Combine whose body recipes name model bodies

**Question.** When the browser-node slots of an operand-list `Combine` bind its tools and the body recipes of the scope name model bodies for the tools, which bodies does the `Combine` use as its tools?

**Known.** `f3d.md` §3.1 gives the slot rule and the body-recipe rules. The slots name bodies of the preceding history state, and the recipes name model bodies. The decoder takes the model bodies and reports `feature.combine-tool-recipe-replacement-assumed` with the slot tools and the replacing bodies. The model bodies are distinct from the target.

**Need.** A Fusion readback of the tool bodies of such a `Combine`, compared with both bindings.

**Conflict.** None.

### DR-561. Destination of a Combine new-component result

**Question.** How does a Combine's new-component request identify the product definition and placement that contain its result?

**Known.** The operand-list prologue stores a new-component Boolean. With that Boolean one, the created-body list of the three-middle-list form names the browser node of each result body in the new component. A two-middle-list request can omit these nodes. The named tail can carry a distinct middle reference; it is separate from that browser node and remains located in the native scope. The Boolean operation and ordered target and tool selections remain readable.

**Need.** A complete identity join from the result browser node and named-tail references to the component and placement, including a request whose middle lists omit the node.

**Note.** The neutral Combine retains the new-component request, and the native operation retains the first created-body node as its destination browser node and the complete created-body list. The decoder reports `feature.combine-component-destination-assumed` for the unresolved destination placement and assumes the input component frame.

### DR-924. Body slot of a pattern copy that a Combine keeps

**Question.** Which body slot of its creating state does the browser node of a body-pattern copy name when that state inserts several bodies and the body is not history-only?

**Known.** `f3d.md` §3.1 binds an operand-list `Combine` tool through the single body that its creating state inserts, or through the history-only body-map pair of its body entity. A consumed pattern copy has a history-only pair and binds as the generated body of the pattern feature. A three-element body pattern makes two copies. When a later `Combine` keeps these copies as tools, the creating state of the copies inserts one body slot for each copy, and the body-map pair of each copy entity is not history-only. The selector of each pair is a key that selects one BREP body record. The index of that record is equal to the slot that the creating state inserts for the same copy.

**Need.** Body patterns whose copies a later feature changes before a keep-tools `Combine`, with the readback of each copy body, to show whether the body record index of the pair selector names the inserted slot in every history.

**Note.** The tools of such a `Combine` stay native and the decoder reports `feature.combine-operand-body-unbound`.

### DR-560. Prologue of a Combine with an expanded named tail

**Question.** Does an expanded named tail select the complete operand-list prologue whenever its fixed prefix also matches the standard prologue?

**Known.** The expanded tail retains references outside the operand table. The complete operand-list reading supplies independent operation, keep-tools and new-component fields. `f3d.md` §3.1 selects the complete operand-list reading of a frame without an expanded named tail when the standard prologue also reads its fixed fields. The standard prefix can match only the initial bytes and give different control values. A component Join can store zero-based operation zero, keep-tools one and new-component one.

**Need.** Independent saves with expanded named tails across Boolean operations and tool-retention modes, including a frame whose complete operand lists contradict the standard-prefix reading.

**Note.** The decoder uses the complete operand-list reading for the expanded named tail and reports `feature.combine-named-tail-prologue-assumed`. The synthetic control test preserves the independent destination node and tail reference while requiring the Join, keep-tools and component-request values.

### DR-202. Fields of the older Shell frames

**Question.** Do the fixed-length `Shell` frames with a Boolean at offset 21 store the fields of the wall-slot form?

**Known.** `f3d.md` §3.1 reads offset 21 as the outward convention in the class-369/261 frame, and the decoder reads the same offset that way in the other fixed-length frames. The wall-slot form stores the tangent-chain Boolean at offset 21 and states the wall direction by which slot is present.

**Need.** Controlled frames of the fixed-length forms that vary the wall direction and the tangent chain independently, with result volumes.

**Conflict.** One byte offset has two meanings across the forms. The two meanings give different wall directions for a tangent-chain selection if the older frames store the newer field.

**Note.** No authored file has a fixed-length frame.

### DR-204. Row default flag and revision members of configuration tables

**Question.** What do a row `default` value `true`, a column `isCustom` value `true`, a nonempty `cachedRevIds` object, and the row `configRevisionId` state in a configuration table?

**Known.** Each row of a populated configuration table stores `default` `false` and a `configRevisionId` UUID that differs from the row id. Every column stores `isCustom` `false`, and `cachedRevIds` is `{}`. These values stay the same when the active row, one parameter cell, one suppression cell, one row name, one row copy, or one column target changes.

**Need.** A saved design with a row marked as the default, a design with a customized column, and a design saved again after changes to configured rows.

**Conflict.** None.

**Note.** The decoder retains these members in the native table document. A `default` or `isCustom` value `true` and each member of `cachedRevIds` emit `configuration.member-unassigned`.

### DR-205. Entries of a modeled extended Thread prefix

**Question.** What do the two u64 values of each 16-byte entry after the entry count of an extended Thread prefix state?

**Known.** A modeled extended Thread stores one entry for each selected face, and a cosmetic extended Thread stores count `0` and no entry. The first u64 values of one thread are `1` on one face, `1` and `6` on two faces, and `1`, `6`, and `7` on three faces. The second u64 values of one thread are consecutive, and they increase through the modeled threads of a design in timeline order. The Design stream has no indexed record with a second value.

**Need.** Modeled threads on four or more faces, and the entity that carries a second value as its identity.

**Conflict.** An extended standard Thread scope with modeled flag `1`, a result state, and helical result faces can store entry count `0`.

**Note.** The decoder admits an entry count of `0` or one entry for each face-selection pair. It reads an extended standard Thread with modeled flag `1` and entry count `0` as modeled and charges `feature.thread-modeled-entries-absent`. It reads the thread class after the entries and keeps the entry bytes only in the retained source image.

### DR-897. Diameter of a fractional Thread nominal size

**Question.** Which diameter does a fractional Thread nominal size state?

**Known.** `f3d.md` §3.1 "The three LP-UTF16 fields are the designation, nominal-size text, and profile name" gives the fraction grammar and states the size in the units of the profile. A pipe thread with nominal size `1/8` stores major diameter `0.9728` cm; the fraction in inches is `0.3175` cm.

**Need.** Pipe threads of several fractional sizes and profiles, compared against the diameter the source application reports for each.

**Conflict.** None.

**Note.** The decoder projects the stored major diameter as the thread diameter and charges `feature.thread-fractional-size-diameter-assumed`.

### DR-206. Span of a two-sided to-object Revolve

**Question.** Which span between its two targets does a two-sided to-object `Revolve` keep, and which neutral extent states that span?

**Known.** `f3d.md` §3.1 "A to-object `Revolve` scope stores extent kind `1`" gives the side-one and side-two target groups. A scope whose two targets are exchanged stores the exchanged groups and produces the same body. For a New Body or Join profile on the XY origin plane revolved about the Y origin axis, the body of a scope with two different targets is a span of the revolution between the two targets, and the profile position lies outside that span. One rule gives the span for plane, face, and body targets: the travel leaves the profile toward the negative-normal side of the sketch plane, the span starts where the travel first reaches the target that it reaches second, and the span ends where the travel next reaches the other target. In that construction, a scope with one plane through the axis as the target of both sides produces the span from the profile position to that plane.

**Need.** Two-sided to-object scopes on another sketch plane or about a reversed axis, scopes with one face or one body as the target of both sides, and Cut or Intersect scopes, each with the resulting body geometry. A neutral extent for a span that starts away from the profile position: the neutral two-sided extent runs from the profile position to one termination on each side of the profile plane.

**Conflict.** None.

**Note.** The decoder keeps both target groups in the native construction and projects a two-sided extent whose two terminations are unresolved.

### DR-172. Paired-locus dimension owner binding

**Question.** How does a paired-locus frame assign its dimension when its explicit owner reference names a later dimension and its paired header is followed by a parameter record?

**Known.** A dimension companion can contain a three-locus frame with a null locus of role `6` and two point loci of roles `0` and `1`. Its return run repeats the two point references in reverse order. The explicit owner reference names the owner of a later dimension whose expression is the name of the companion's dimension. The paired header is followed by the later dimension's parameter record and then its owner. The decoder retains the companion as a native dimension operand. The two distance constraints of the later dimension have neutral operands.

**Need.** Controlled linear dimensions with and without a dependent dimension must establish the relation between the containing companion, the explicit owner reference, and the following parameter and owner records.

**Conflict.** None.

**Note.** None.

### DR-240. Presentation frame whose label states another value

**Question.** Which dimension does a direct presentation frame govern when its label states a value that differs from the first following dimension owner's parameter?

**Known.** The opaque presentation bytes of a direct frame hold an LP-UTF16 label that states the governed parameter's expression. Under the first-following-owner rule, one parameter owner can receive two frames, and a frame can receive an owner whose parameter differs from its label. When one owner has several frames, the frame whose operand measurement equals the parameter governs it.

**Need.** Files with the direct two-operand frame form that create a dimension, delete or replace it, and save the document after each change.

**Conflict.** None.

**Note.** A frame can state `91.675 mm` with its two points 91.675 mm apart while its first following owner is a 5.175 mm parameter that a second frame also selects.

### DR-241. Flag byte of the Base Feature marked null reference

**Question.** What does the flag byte of the marked null reference in a `Base Feature` result-body envelope state?

**Known.** `f3d.md` §3.1 "A `Base Feature` scope records a result-body snapshot" gives the marked null reference of the ordinary form. Ordinary frames written by earlier releases store `0`. A 318-byte class-348/257 ordinary frame can store `1` and repeat its passive reference. One-body 331-byte ordinary frames from the current Fusion release, whose body is built in place, store `1` and repeat their body entity suffix. A 316-byte class-368/258 suffix result-body frame, whose body is a copy of a body in a referenced design, stores `1`. The decoder reads the result runs of a flagged ordinary or suffix result-body frame as the ordinary result set and reports `feature.base-feature-null-flag-assumed`. Layout record `base_feature_wide_zero_body` in [the layout tables](../layouts/f3d.md) defines offsets 21 through 35 of the 262-byte wide zero-body form as zero; offsets 20 through 30 there have the layout of a marked null reference. A 262-byte wide zero-body frame can store `1` at offset 29 with equal current and preceding history states, or `0`. A 275-byte wide zero-body frame that holds a mesh body stores `1` at offset 29. The decoder keeps the wide zero-body frames with flag `1` as unbound stored-body scopes.

**Need.** Saves of one design that differ only in this byte, for example across Fusion releases or design options.

**Conflict.** None.

**Note.** DR-483 tracks a marked null reference whose byte after the flag byte is `1`. Unbound result references remain a native body selection of a neutral Base Feature and report `feature.stored-bodies-unbound`.

### DR-1018. Flag byte before the Base Feature metadata record

**Question.** What does the flag byte before the shared metadata record of an ordinary `Base Feature` result-body frame state, and does flag `1` with empty modern body maps always identify the snapshot body as the model body?

**Known.** `f3d.md` §3.1 "A `Base Feature` scope records a result-body snapshot" places the byte after the repeated run. A `Base Feature` as created can store `0` there. A save after an Edit Feature on that `Base Feature` can store `1` and have no history-bearing BREP, empty modern body maps, and a snapshot body map whose one pair names the body entity suffix of the frame. The decoder selects the snapshot `.smb` set as the model body set in a stream whose modern maps all have zero pairs and empty basenames, that holds a `Base Feature` ordinary result-body frame with flag byte `1`, and whose snapshot maps hold at least one pair with every pair naming a body entity suffix of such a frame. A flagged frame with populated modern body maps keeps its modern selection.

**Need.** Further saves of designs whose `Base Feature` was edited after creation, with their modern and snapshot body maps, and saves of designs with a flagged frame and a populated modern body map.

**Conflict.** None.

**Note.** The decoder reports this selection with `design.flagged-base-feature-body-selected`. The `Base Feature` of such a design has no history partition, so its output state chain stays unbound and the decoder reports `feature.output-state-chain-unbound`.

### DR-646. Entry order of a multi-body suffix result-body frame

**Question.** How does a `Base Feature` suffix result-body frame with more than one body order its body, repeated, and result entries?

**Known.** `f3d.md` §3.1 "A `Base Feature` scope records a result-body snapshot" gives the one-body suffix result-body form. The ordinary form stores each entry kind as one consecutive run. The decoder reads a frame with u32 `N` greater than `1` at offset 20 and its kind at offset `169 + 37N` as `N` body entries, the marked null reference, u32 `N`, `N` repeated entries, one zero byte, the metadata record, u32 `N`, and `N` result entries, and reports `feature.base-feature-suffix-run-order-assumed`.

**Need.** A controlled save whose Base Feature copies two bodies from a referenced design.

**Conflict.** None.

### DR-251. Compact parameter ownership and expression trailer flags

**Question.** Does the null owner of a compact null-owner dimension parameter mean that the parameter has no owner or that the frame does not store it, and what does byte 3 of that trailer state when it is `1`? What does byte 0 of the ordinary compact owned trailer state?

**Known.** The specification states the compact null-owner parameter form and the class-`287` form, which both admit the trailers `00 00 00 00 00` and `00 00 00 01 00`. Linear Dimension parameters in the compact null-owner form can store `00 00 00 01 00`; a run of five such parameters chains its expressions (`6.35 mm`, then each names the previous one). The compact owned trailer stores the driven flag at byte 1, which aligns with byte 6 of the wide expression trailer; under that alignment, byte 3 is the position of the wide favorite flag. These frames store the null owner `u8 0` at offset 30 while their source kind names a Linear Dimension. The decoder reads them without an owner binding and reports `parameter.owner-unstored` for each stream with such a parameter. It keeps the parameter flags at their defaults and reports `parameter.trailer-flag-uninterpreted` for each stream with a decoded parameter whose byte 3 is `1`. Ordinary compact owned `Side1Offset`, `TangencyWeight`, `Linear Dimension`, and `Diameter Dimension` parameters can set byte 0 with the driven flag clear. The decoder retains such a `Side1Offset` and reports the same loss, and it does not decode the parameters of the other source kinds.

**Need.** Fusion's own reading (favorite, driving state, owner) of compact null-owner parameters whose trailer is `00 00 00 01 00`, or a further design with the compact null-owner form or a compact trailer that sets byte 3. A controlled owned parameter save must identify the setting that changes byte 0.

**Conflict.** None.

### DR-175. Ownerless fixed Fillet lanes across several operand groups

**Question.** How do ownerless fixed scalar lanes assign radius laws to several role-`0x0000000800000000` Fillet groups?

**Known.** The scalar records name the owning scope and have consecutive owner-local ordinals. Their scope reference table interleaves the scalar records with several counted operand groups. A scalar run can contain two values with two groups, or three values with three groups. The fixed single-group grammar reads these runs as a tangency weight and constant radius, or a tangency weight and two endpoint radii.

**Need.** A record discriminator or explicit assignment that separates per-group radii from a shared tangency weight and a multi-value radius law.

### DR-166. Pathless occurrence-placement control word

**Question.** What does the u32 after the empty path and separator in a pathless occurrence placement select?

**Known.** A pathless placement can store value `1` in the 25-byte prefix. Its two metadata UUIDs can equal the UUIDs of the corresponding repeated-target placement in another assembly. Its typed target, role, rigid matrix, and local owner close the placement frame. The decoder admits control value `1` and retains the complete frame.

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

**Conflict.** The Body container and terminal ASM state can retain ten bodies in addition to the five bodies of the producer's current view and STEP export. Every record of the registered whole-body selection family in such a stream decodes, and its two Remove selections name two distinct browser nodes that the consuming-scope rule already covers.

**Note.** Preserve bodies while the browser-entry state and timeline endpoint remain unresolved. Creator family and container order supply comparison groups for the controlled saves.

### DR-242. Thread face continuation through later topology rewrites

**Question.** Which stored relation carries a Thread selection from its result face to the selected region after a later operation rewrites that face?

**Known.** The ASM transition chain preserves stable entity slots across record updates. A later transition can retain the threaded cylinder's slot, axis, and radius while it updates that face, deletes another face, and inserts a replacement face. The Thread selection names its preceding topology. When every following transition keeps the selected face slot, its surface binding, and the bound surface record and its surface rows, the selected slot identifies the active face in the same BREP member (`f3d.md` §3.1 "A Thread input-face identity uses the stable entity slot").

**Need.** Controlled Thread designs followed by a split of the threaded face onto a new carrier, deletion of the threaded face, and motion of the threaded face, with the selected threaded regions identified before and after each operation.

**Conflict.** None.

**Note.** A following transition that drops the slot, rebinds its surface, or changes the bound surface record or its rows leaves the face unresolved with `feature.thread-live-face-unproven`.

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

**Need.** Documents with controlled metadata changes and the same terminal form.

**Conflict.** The byte sequence permits several scalar partitions.

**Note.** The reader preserves the complete manifest in the native archive payload.

### DR-249. Saved assembly state and joint activation

**Question.** Which saved assembly state selects the occurrence poses for an Assemble joint relation, and what activation state do the two scope-tail booleans encode?

**Known.** The version-three connector equation uses scope frames, four referenced angle and XYZ lanes, and facing. Ordered occurrence keys select the innermost child within one insertion, including inserted documents. A projected joint instance can keep a consumer-pose residual above 0.1 mm, or exceed that threshold under current carriers. An incomplete binding can be an open relation or a closing relation. A joint record can store `01 01` in the two tail booleans; such records retain native alignment, and their relations can close. A version-three record can store terminal bytes `00 00`; a version-four record can store an extra preceding `01` marker followed by `00 00`. Projection assumes every projected Assemble joint is active, supplies `suppressed = false` independently of these bytes, and charges `assembly.joint-activation-assumed`.

**Need.** A byte-level saved-state selector with links from joints to occurrence-pose carriers. Saved states that change each tail boolean independently and establish the joint's activation state across independent designs.

**Conflict.** The full-path audit retains consumer-pose residuals of 17.8801954, 42.9906967, 40.0011250, 373.6835274, and 373.6835274 mm on joint relations. A relation can have a 6.50691939 mm current-carrier residual and a consumer residual below 0.0001 mm. Two relations whose records store `01 01` in the tail booleans have residuals of 40 mm and 18.35 mm. An admitted relation has a current-carrier residual of 0.0405289814 mm.

**Note.** Scope-level closure counts describe selected carrier pairs. Emitted-joint closure uses the actual neutral occurrence containers and complete insertion paths. Nonzero tail states preserve the decoded alignment in a projected relation with assumed operand contexts and active state. Admission uses byte grammar and binding evidence independently of pose closure. A relation whose inserted occurrence stores the identity can have a 1472.38643012 mm current-carrier residual; DR-116 covers it. DR-113 and DR-116 cover their separate placement questions.

### DR-250. Motion of a joint record shared by several Assemble scopes

**Question.** When several version-3 `Assemble` scopes reference one joint record, which scope does its motion record bind, and do the other scopes take the same freedoms, limits, and rest state?

**Known.** An adjacent joint record has an index one less than its scope's, and its motion binds that scope. Several scopes can reference one earlier joint record in a design whose Design streams store no freedom entry, so the shared joint record shows no motion.

**Need.** A design whose shared joint record has a motion record with freedom entries, with the moving scope identified.

**Conflict.** None.

**Note.** Scopes with decoded connector frames project a neutral relation that preserves their facing, angle, and offsets. Adjacent decoded motion supplies the motion when available; the relation is otherwise fixed. `assembly.motion-binding-assumed` records this binding assumption.

### DR-248. Legacy closing references and six-name placement bindings

**Question.** How does the closing reference of a 531-byte legacy placement select an owner other than target `4`, and how do the instance key and metadata UUIDs bind the six-name envelope?

**Known.** `f3d.md` §1.4 "The empty-attribute generation can insert" defines independent source and local placements in the attributed envelope. "A leading-flag local component occurrence" defines the corresponding local owner carrier. In a pair of saves of a construction that places two inserted parts 10 mm along each owner's local X axis, Owner B is nested inside Owner A. Owner B's CommonData path and display row agree on its world frame. Capturing Owner A's translation changes its closing local matrix and moves both inserted parts by `(20, 30, 50)` mm. The inserted-part source and local matrices stay equal across this pair. A role-matched 531-byte matrix carrier can have a non-null closing target other than `4`. A root can contain plain version-2 placement carriers with an extra byte before the metadata count, and version-2 six-name matrix carriers. The six names are `Appearance`, `Geometry`, `PhysicalMaterial`, `Properties`, `StructureChange`, and `Transform`.

**Need.** Close each complete plain or six-name indexed span through its registered type, occurrence role, and local owner. Vary the inserted child's local placement and selected captured state separately, with an independent scene or joint frame. Bind the six-name instance key and both metadata UUIDs through controlled repeated insertions, tracked-channel changes, and joint operand paths. A 531-byte legacy carrier with a closing target other than `4` requires an independent owner-relative pose.

**Conflict.** None.

**Note.** Equal source and local matrices establish the child frame of this nested construction. The event represented by a differing inserted-part source frame remains open.

### DR-252. Saved scene framing and state selection

**Question.** How do enclosing node and attribute frames delimit the saved world stream, which saved state do displayed overrides represent when their poses differ from Design carriers, which scene paths descend beyond the neutral occurrence tree, and what does the tagged path suffix name?

**Known.** Transform rows close through their complete local grammar in `f3d.md` §1.1.4. A world stores transform rows in its root scene and in member scenes. A world can also store `OverrideInActivatedVisibilityAttribute` with a path of five elements. `FOverrideProteinEffectAttribute` names can occur in the same Component serializer region as transform rows. Nested world objects contain `WorldSerializerEndMark` strings. Override paths can store a tagged pair suffix of up to 14 elements. The low indices of these pairs match no carrier of a neutral occurrence under the untagged prefix.

**Need.** Controlled saves that vary one node attribute, the active captured position, or one occurrence depth at a time. Pair each stored scene with its Design carriers and complete serializer boundaries.

**Conflict.** Displayed matrices and stored Design world poses can differ.

**Note.** Enclosing node framing is not decoded. The local transform reader leaves visibility and material override attributes unread. Binding uses the root design scene; member scenes retain their separate saved state.

### DR-253. Typed occurrence path identity-pair words

**Question.** What does the u32 pair word after each u32 `2` in a positive-count typed occurrence path state, and how does it affect binding?

**Known.** `f3d.md` "Assembly operands" gives the two pair words of a typed path record. Each word is `0` or `1`, and the records of one continued path store the same pair. Live version-4 paths of type `96CBEA21-17E0-4BCB-BA77-04EA87B47934` can store `(1, 1)` (encoded as `0x0000000100000002` between the two GUID pairs and u32 `1` after the second `u32 2`); other typed paths store `(0, 0)`. Each path has one or two occurrence keys and four identity GUIDs. Each 425-byte path is bounded by its indexed wrapper. The `(1, 1)` paths belong to 806-byte Assemble scopes whose occurrence keys resolve and whose connector frames at current occurrence poses satisfy the joint relation in "Assembly operands". The decoder reads the occurrence and identity GUIDs of a path with word `1` as it reads them with word `0`, projects the joint, and charges `assembly.operand-path-word-assumed`.

**Need.** Make controlled saves of one joint between two fixed component occurrences. Give each occurrence an identity-transform child with a coincident connector. In separate saves, change the first operand from its root connector to its child connector, then change the second operand in the same way. Hold both connector matrices fixed. In further independent saves, update only the first component revision, only the second component revision, and only the joint suppression state. Create two component configuration rows with equal geometry and equal connector matrices; switch each operand between those rows independently. Compare both pair words and all five GUID fields with the baseline. Obtain both `(0, 0)` and `(1, 1)` and establish which controlled changes affect each word so a writer knows when to emit word `1`.

**Conflict.** None.

**Note.** The decoder projects joints backed by paths with word `1` and charges `assembly.operand-path-word-assumed`.

### DR-255. Decal target selected by a face-recipe entity selection

**Question.** What does a Decal target group carry when a member is a persistent entity selection whose identity record holds a face recipe instead of a whole-body recipe envelope, and which faces does it select?

**Known.** `f3d.md` §3.1 "A `Decal` scope is a presentation timeline object" gives the scope prefix layouts and a target group with role `0x0000000400000000` whose whole-body recipe envelope members identify faces. A target group can have a member `N` of another form. The records `N` through `N + 3` follow the entity-selection frame: a prefix with an asset GUID and a context GUID, the paired header `N`, and the nested records `N + 1` and `N + 2`. The identity record `N + 3` is 90 bytes long and contains a `face_recipe_data` program. Record `N + 4` stores the same asset and context GUIDs. The entity-selection frame grammar admits 29-byte and 45-byte identity records and the 49-byte class-338 sketch-curve identity with a class-361 identity record, so this member decodes as neither an entity-selection operand nor a body-recipe operand.

**Need.** Make controlled saves of one Decal on one planar face and on one cylindrical face. Compare the target member records with the whole-body recipe form. Establish the grammar of the 90-byte identity record and of record `N + 4`, and how the face recipe resolves to faces.

**Note.** For a Decal with no decoded placement record, the decoder projects the Decal with its image and a native face selection and records `appearance.decal-target-unresolved`. DR-484 covers a Decal with a decoded placement record.

### DR-484. Decal placement record fields and the scope code

**Question.** What do the Decal scope code, the three bytes after `V`, and the unnamed placement-record fields select? Does the second of the three bytes, or the direction of `N` against `U x V`, state a mirrored image? Which placement records store adjacent trailing references or five zero bytes before `N`? Which direction of `V` is the top of the image? What is the placement-record layout of a typed Decal?

**Known.** `f3d.md` §3.1 "The image-asset record index also names a Decal placement record" gives the placement record. The scope code between the image-asset reference and the target-group reference stays `0x01` while the decal moves, rotates, flips, changes scale, and changes from a planar face to a cylindrical face. Decal scopes also store the codes `0x00`, `0x60`, `0x7D`, and `0xE2`. Reversing the decal x axis negates `U` and leaves the three bytes after `V` at `01 00 00`. The decoder keeps the three bytes in the native `flag_bytes` field. `N` has the direction of `U x V` on the default, moved, scaled, and cylindrical placements and the opposite direction when the decal x axis is reversed. The decoder keeps a placement record whose `N` is at an angle to `U x V` and reports `appearance.decal-placement-normal-assumed`. The two f64 values after the first trailing reference are the in-plane coordinates of `O` on a planar face and `(0.25, 0)` on a cylindrical face; the f32 value is `1.0` at opacity `1.0`. The decoder also reads a placement record whose trailing references are adjacent or that stores five zero bytes before `N`, at the one position that closes the record, and reports `appearance.decal-placement-layout-assumed`. The decal y axis has the direction of `V`. The decoder keeps the frame of a decoded placement record in the native `placement` field, names the image asset, the target group, and the resolved target faces in the feature source properties, leaves the feature definition native, and records `appearance.decal-placement-native`. A Decal with no decoded placement record takes the fit-to-faces mapping with `appearance.decal-mapping-assumed`; every typed Decal is in that group.

**Need.** Controlled saves of one Decal with the dialog flip controls, an opacity other than `1.0`, and face chaining, and one save from a file generation that writes typed references. Compare the scope code, the three bytes after `V`, the unnamed fields, the zero runs around the trailing references, and the rendered image orientation across the saves.

**Conflict.** None.

**Note.** The neutral placement frame needs the pixel size of the image and a neutral Decal placement field.

### DR-256. Ordinal prefix of a leading-flag local component occurrence

**Question.** What does the u16 ordinal prefix at offset 44 of a leading-flag local component occurrence state?

**Known.** The prefix is `1` for the first new component of a Design and for a pasted second instance of that component, and `0` for a later new component. Its value is independent of the occurrence's grounded, light-bulb, visibility, active, and isolated states. The attributed frame stores two bytes after its attribute run, before its ordinal.

**Need.** A Fusion control that changes the prefix of one occurrence with all other carrier fields equal.

**Conflict.** None.

**Note.** The decoder keeps the prefix in the native occurrence frame.

### DR-268. Loft setting of the standard Loft scope marked reference

**Question.** Which Loft setting selects a non-null marked reference at standard `Loft` scope offset 33?

**Known.** `f3d.md` §3.1 "The compact fixed-prefix `Loft` scope has ten zero bytes" gives the reference syntax, and §3.1 "A `Loft` has at least two sections" makes the group that the target names a rail. In each scope with a non-null reference, the scope has one role-`0x0000000500000000` rail group and the target equals the record index of that group. Scopes with rail groups or a centerline group also store a null reference. The decoder keeps the target in the Loft construction, records `feature.loft-reference-unresolved`, and projects the Loft with closed false, ruled false, and linearize false.

**Need.** Saves of one railed `Loft` that change one rail or guide setting at a time, each recording byte 33 and the target, and a save whose target is not a record of the scope's reference table.

**Conflict.** None.

**Note.** None.

### DR-294. Flag bytes of the three-flag Loft prefixes

**Question.** Which Loft settings do the three flag bytes at offsets 21 through 23 of the three-flag compact `Loft` prefix and at offsets 25 through 27 of the three-flag standard `Loft` prefix select?

**Known.** `f3d.md` §3.1 "The compact fixed-prefix `Loft` scope has ten zero bytes" gives the flag positions. The three-flag compact form stores `01 01` and a byte `0` or `1`. The decoder records `feature.loft-settings-assumed` with the three flag bytes for each scope in either form and projects the Loft with closed false, ruled false, and linearize false.

**Need.** Saves of one three-flag `Loft` that change closed, solid, ruled, and linearize one at a time, each recording the three flag bytes.

**Conflict.** None.

**Note.** None.


### DR-331. Fields of the Loft scope body

**Question.** Which Loft settings do the head words, the three references, the selector, and the six bytes between the rail list and the third reference of the `Loft` scope body select, what does the section record `N` of a section-list entry store, and which stored field makes a `Loft` a surface Loft?

**Known.** `f3d.md` §3.1 "The `Loft` scope body starts after the result-operation u32" gives the field order. The first head word is `-1` or `1`. A nonzero selector precedes a non-null second reference, which names a parameter owned by the scope. The decoder reads the rail list and the section list, takes construction-operand group `N+1` as the section group of section-list entry `N`, keeps the other fields in the scope bytes, and projects the Loft with closed false, ruled false, and linearize false. The four standard-prefix flag bytes at offsets 25 through 28 can all be `01` in a new-body Loft whose output body is a sheet body. The decoder decodes the output bodies of each projected Loft at the Loft scope's history state. When a body does not decode there, the decoder reads the output bodies of the final model. It projects a new-body Loft whose output bodies are all sheet bodies with solid false and every other Loft with solid true, and records `feature.loft-solid-inferred` with the body source for each projected Loft.

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

**Question.** Which Thicken setting does the prologue flag byte of the optional-flag prologue of a `Thicken` scope select?

**Known.** `f3d.md` §3.1 "A `Thicken` scope primary frame stores nine zero bytes" gives the byte position and the two settled flag bytes. Saves that change the thickness sign, the symmetric direction, and tangent-chain selection store prologue flag byte `0`. The decoder records `feature.thicken-flags-assumed` when the byte is `1` and projects its decoded result, symmetric flag, tangent-chain flag and thickness.

**Need.** Saves of one `Thicken` that change one further setting at a time and store prologue flag byte `1`.

**Conflict.** None.

**Note.** None.

### DR-314. Optional u32 zero of the Thicken scope frame

**Question.** What does the u32 zero that can follow the marked scalar reference of a `Thicken` scope encode, and which scopes store it?

**Known.** `f3d.md` §3.1 "A `Thicken` scope primary frame stores nine zero bytes" gives the field and the three counted reference runs that follow it. The decoder reads scopes with and without the field and keeps no value from it.

**Need.** `Thicken` scopes that store a nonzero value in the field, or saves of one `Thicken` from application versions whose frames differ in the field.

**Conflict.** None.

**Note.** None.

### DR-503. Thicken identity runs and incomplete construction references

**Question.** Which entities do the first counted prefix run and unmatched Thicken prefixes identify, and how do incomplete group definitions select their operands?

**Known.** Result words `1` through `4` select join, cut, intersect and new-body construction. The third counted run selects a Boolean body group. The first run retains its reference indices on the native Thicken record. The decoder projects an unmatched prefix with `feature.thicken-frame-assumed`. Missing or ambiguous scalar definitions retain the result, flags and groups with `feature.thicken-thickness-unresolved`. Incomplete input and target groups retain their scope expressions with `feature.thicken-face-selection-assumed` and `feature.thicken-target-selection-assumed`. Zero or overflowing converted thickness retains its source scalar with `feature.thicken-thickness-unrepresentable`.

**Need.** Controlled saves that change the first-run identities and reference definitions independently, plus additional closed prefix forms.

**Conflict.** None.

**Note.** A retained group can bind to an earlier body recipe. A retained scope expression keeps the decoded construction available when that binding remains unresolved.

### DR-269. Marker-one Revolve frames of other lengths

**Question.** Which field layout does a `Revolve` scope use when it has the marker-one prologue and a token-independent frame length other than 345, 355, 367, 351, or 373?

**Known.** `f3d.md` §3.1 gives the marker-one form a 345-, 355-, or 367-byte token-independent frame, and the referenced-angle form, which shares the marker-one prologue through offset 32, a 351-, 361-, or 373-byte frame. The decoder reads a frame of another length with the marker-one field offsets and records `feature.revolve-layout-assumed`.

**Need.** A Design stream with a marker-one `Revolve` scope of another token-independent length and the feature's parameters as the application shows them.

**Note.** None.

### DR-657. Referenced-angle Revolve direction kind 1 and second tail reference

**Question.** What does direction kind `1` at offset 33 of a referenced-angle `Revolve` scope select? What does the second marked reference of the 88-byte tail of a 361- or 379-byte referenced-angle frame name?

**Known.** `f3d.md` §3.1 gives the referenced-angle form direction kind `0` or `1`, and the 361- and 379-byte frames the 88-byte fixed tail. Direction kind `1` occurs with a full turn, where the swept solid is the same for each direction. The decoder reads direction kind `1` only for a full turn and leaves a partial-turn scope with direction kind `1` unresolved. The second tail reference names a record index of the same Design stream that is not an ordered scope reference. The decoder reads the 361-byte frame with the 351-byte fields and the 379-byte frame with the 369-byte fields, and records `feature.revolve-layout-assumed` with the target.

**Need.** One referenced-angle `Revolve` saved with a partial turn in each direction, once with direction kind `0` and once with direction kind `1`, and 361- and 379-byte referenced-angle scopes with the records that their second tail references name.

**Conflict.** None.

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

### DR-473. Geometry states of mesh operation scopes

**Question.** Which tessellation does each output-geometry and input-geometry reference of a mesh operation scope select?

**Known.** Each reference names the Design GUID record of one mesh geometry state. The source group names the mesh bodies of the operation. The decoder keeps the scope with the ordered body record indices as the mesh source and stores both GUID record indices in the source properties.

**Need.** The join from the GUID record of a superseded geometry state to its `.paramesh` container, checked against a controlled operation whose input and output meshes are known.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-operation-source-unresolved` and `feature.mesh-plane-cut-source-unresolved` for historical geometry. Plane-cut admission, framing, plane components and selection membership report `feature.mesh-plane-cut-scope-unparsed`, `feature.mesh-plane-cut-tail-partial`, `feature.mesh-plane-cut-plane-partial` and `feature.mesh-plane-cut-selection-unresolved`. Unruled controls and retained construction fields report `feature.mesh-plane-cut-selector-unruled` and `feature.mesh-plane-cut-fields-uninterpreted`. An unreadable scope or source group reports `feature.mesh-operation-scope-unparsed`. A partially decoded geometry tail reports `feature.mesh-operation-tail-partial` and retains the preceding controls and references.

### DR-474. Mesh operation selector values and fixed fields

**Question.** Which methods do `MeshRepair` method selector `2` and rebuild selectors `1` and `3` name, and what do the fixed zero fields of the `MeshReduce` and `MeshRemesh` scopes and the u32 `1` after the `MeshRemesh` edge-length owner store?

**Known.** Method `0` closes holes, `1` stitches and removes, and `3` rebuilds. Rebuild selector `0` is fast and `2` is accurate. The decoder stores every selector value in the source properties and leaves the neutral field of an unlisted value unset. The decoder stores a fixed field with another value in the source properties.

**Need.** Controlled saves of each repair method and rebuild type, and of remesh and reduce operations with each dialog option changed alone.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-operation-selector-unruled` and `feature.mesh-operation-fields-uninterpreted`. An owner reference without a usable value reports `feature.mesh-operation-parameter-unbound`.

### DR-475. Section curves of a mesh section

**Question.** Which records of the referenced `Sketch` scope store the section curves of a `MeshSection`?

**Known.** The `MeshSection` scope references the `Sketch` scope that receives the section curves. The decoder names the feature of that scope in the neutral `MeshSection`.

**Need.** A mesh section sketch with a known section polyline, with its sketch records compared against a sketch without section curves.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-section-curves-unresolved`. An unavailable section plane reports `feature.mesh-section-plane-unresolved`.

### DR-504. Mesh operation member selection tails

**Question.** Which source member tail fields select face groups or subsets of a mesh body?

**Known.** A source member contains a body reference followed by selection fields. The decoder retains the body reference as an input-local body selection.

**Need.** Controlled saves with whole-body and face-group selections changed independently.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-body-selection-assumed` for the body-selection reading and `feature.mesh-plane-cut-membership-assumed` for plane-cut source membership.

### DR-505. Mesh shell construction controls

**Question.** Which mesh shell fields encode the wall direction, offset construction, corner continuation, and intersection settings?

**Known.** The mesh shell scope names its thickness owner and source mesh. These values define the neutral Shell input and thickness.

**Need.** Controlled saves that change each shell construction control independently.

**Conflict.** None.

**Note.** The decoder leaves unresolved construction settings unset and reports `feature.mesh-shell-controls-unresolved`.

### MC-01. Mesh conversion controls

**Question.** Which conversion control selects the organic method, and which construction settings do the wrapper control fields select?

**Known.** Conversion control `1` selects the prismatic method and `3` selects the faceted method. An output wrapper stores a control byte before its body reference. The decoder retains the raw values in source properties and projects an ordered mesh-to-body conversion with its readable method.

**Need.** Controlled saves with faceted, prismatic, and organic conversion methods and construction settings changed independently.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-conversion-method-assumed` for the control-to-method reading and `feature.mesh-conversion-method-unresolved` for an unruled control. A truncated scope reports `feature.mesh-conversion-scope-unparsed` or `feature.mesh-conversion-tail-partial` and retains readable controls and inputs.

### MC-02. Mesh conversion output envelope

**Question.** Which format discriminator selects the optional four-byte control before the output count?

**Known.** The two output-list forms contain two or six control bytes after the fixed u32. The output list is followed by eight zero bytes and the counted scope GUID. The decoder reads the unique envelope with this following prefix and retains the optional word in source properties.

**Need.** Controlled saves across the producing application versions and operation settings that select each envelope.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-conversion-envelope-assumed` for this envelope reading.

### MC-03. Mesh conversion geometry identities

**Question.** Which historical tessellation and neutral result body does each conversion wrapper identify?

**Known.** Each input wrapper names one mesh operation group. Each output wrapper stores a body GUID and body-record reference. The decoder retains their order and references. Each input uses its declared scope as its native selection source. Source properties retain its wrapper context and readable group-member and body references, including references with unavailable definitions. A partial output wrapper retains its readable GUID and control.

**Need.** A controlled conversion with known input tessellations and output body identities, joined through the wrapper records and historical geometry states.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-conversion-source-unresolved` and `feature.mesh-conversion-output-unresolved`. DR-504 tracks input member-tail selections.

### MC-04. Mesh combination selector three

**Question.** Which result structure does mesh combination selector `3` create?

**Known.** The selector follows the mesh scope prefix. The decoder projects selector `3` as a compound with target members followed by tool members.

**Need.** Controlled saves that distinguish preserved interior structures, ordered compound members, and the resulting mesh-body identity.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-combine-merge-assumed`.

### MC-05. Mesh combination retention and destination

**Question.** Which fields select tool retention and the result component of a mesh combination?

**Known.** The version-2 prefix contains an additional control word. Both versions contain a control byte. Boolean projection assumes consumed tools and the target component. Compound projection retains the controls with unresolved retention and destination.

**Need.** Controlled saves that change Keep Tools and New Component independently, with both scope versions.

**Conflict.** None.

**Note.** Boolean projection reports `feature.mesh-combine-keep-tools-assumed-false` and `feature.mesh-combine-destination-assumed-target-component`. Compound projection reports `feature.mesh-combine-keep-tools-unresolved` and `feature.mesh-combine-destination-unresolved`.

### MS-01. Mesh separation method selector

**Question.** Which Separate selector values identify shell and face-group partitioning?

**Known.** Shell-correlated scopes store selector 0. A face-group-correlated scope stores 65536. The decoder provisionally reads 0 as Shells and 65536 or 131072 as FaceGroups, preserving the raw selector.

**Need.** Matched saves that change each method alone and distinguish method encoding from selection state.

**Conflict.** Labels and body counts also vary between these scopes. The 131072 reading extrapolates the dialog choices. Separate control bytes before a narrower method field also fit the raw words.

**Note.** The readings report `feature.mesh-separate-shells-method-assumed` and `feature.mesh-separate-face-groups-method-assumed`. Other values report `feature.mesh-separate-selector-unruled`.

### MS-02. Mesh separation face-group scope

**Question.** Which Separate fields select all groups or an ordered subset of groups?

**Known.** The decoder provisionally reads selector 65536 as All and 131072 as Selected with unknown members. Source member body references and UTF-16 identifiers remain independent native facts.

**Need.** Matched All and Selected saves with known, independently varied group identities and historical triangle assignments.

**Conflict.** An independent scope field or member-tail selection could carry the selection category. The unseen 131072 reading follows UI choice order while retaining the observed high-word spacing.

**Note.** The readings report `feature.mesh-separate-all-groups-assumed` and `feature.mesh-separate-selected-groups-assumed`. DR-504 tracks general member-tail selection semantics.

### MS-03. Mesh separation Multiple Bodies carrier and polarity

**Question.** Which bytes encode Multiple Bodies, and which value enables it?

**Known.** The stored eight-byte lanes hold zero bytes. The decoder provisionally reads its first little-endian u32 as Multiple Bodies, with 0=false and 1=true, including inactive stored values. A different word leaves this control unknown and preserves the word.

**Need.** Matched face-group saves that vary Multiple Bodies alone, including the inactive Shells state.

**Conflict.** Reserved or count words, narrower flags, reversed word order or polarity, and different controls fit the same lane.

**Note.** The readings report `feature.mesh-separate-multiple-bodies-carrier-assumed` and `feature.mesh-separate-multiple-bodies-polarity-assumed` independently. Unsupported values report `feature.mesh-separate-selector-unruled`.

### MS-04. Mesh separation Keep Body carrier and polarity

**Question.** Which bytes encode Keep Body, and which value enables it?

**Known.** The decoder provisionally reads the lane's second little-endian u32 as Keep Body, with 0=false and 1=true. Its raw bytes and an inactive stored value survive independently of Multiple Bodies and the method.

**Need.** Matched saves that vary Keep Body alone across the separation methods and Multiple Bodies states.

**Conflict.** Reserved or count words, narrower flags, reversed word order or polarity, and different controls fit the zero-filled lane.

**Note.** The readings report `feature.mesh-separate-keep-body-carrier-assumed` and `feature.mesh-separate-keep-body-polarity-assumed` independently. Unsupported values report `feature.mesh-separate-selector-unruled`.

### MS-05. Flagged Separate prefix applicability

**Question.** Does Separate flag 1 insert the common mesh-family u32 prefix word?

**Known.** Separate scopes establish flag 0 framing. The decoder provisionally admits flag 1 through the mesh-family insertion rule and retains the word.

**Need.** A Separate save with flag 1 and checked scope, count and tail bounds.

**Conflict.** A Separate-specific flag meaning could move the later fields differently.

**Note.** This extrapolation reports `feature.mesh-separate-prefix-assumed`. DR-372 tracks the inserted word's meaning.

### MS-06. Separate reference roles and neutral identities

**Question.** Which native references identify produced bodies, whole-mesh inputs and common-table associations?

**Known.** Counted body records reference the Separate scope; source-group members reference different mesh bodies. The common table repeats the source group and a member. The decoder provisionally reads output and input roles, retains every slot position and raw envelope, and preserves both references on a disagreement.

**Need.** Controlled source and result membership changes with independent body-identity and history joins.

**Conflict.** Stored reference runs can describe cached or intermediate states. Native record identities and counts alone do not establish neutral result ownership.

**Note.** The readings report `feature.mesh-separate-output-role-assumed`, `feature.mesh-separate-source-membership-assumed` and `feature.mesh-separate-reference-roles-assumed`. Unresolved output ownership reports `feature.mesh-separate-output-unresolved`. Reference failures and repetition disagreements report `feature.mesh-separate-reference-unresolved` and `feature.mesh-separate-reference-mismatch`. Incomplete or invalid native source membership reports `feature.mesh-separate-membership-unresolved` independently of historical geometry binding.

### MS-07. Separate group and member auxiliary fields

**Question.** What do the Separate auxiliary references, group role, two native identifiers and member words encode?

**Known.** The group carries an auxiliary reference and a u64 role. A member carries an auxiliary reference, a word, two UTF-16 identifiers and two further words. The final word varies independently between source members. The decoder retains these fields and bounded raw records.

**Need.** Matched saves changing whole-mesh selection, selected groups, controls and serialization generation independently.

**Conflict.** State, selection detail and serialization generation can each explain the varying word. Body references and these identifiers do not yet establish selected face-group names.

**Note.** The decoder reports `feature.mesh-separate-source-fields-uninterpreted`. DR-504 tracks selection-tail semantics across mesh operations.

### MS-08. Separate historical source geometry

**Question.** How do Separate source-member bodies bind to historical tessellations and face-group assignments?

**Known.** Complete admissible native membership yields an ordered input-local selection under MS-06. Selected scope can retain unknown members. Face-group definitions remain incomplete until their historical assignments and identities are bound. Shell definitions require actual tessellation selections and known Keep Body; stored group controls are inactive.

**Need.** A history-aware join from source member records to geometry states and triangle-group identities.

**Conflict.** Current displayed geometry can differ from the operation's input state.

**Note.** The decoder reports `feature.mesh-separate-source-unresolved`. Unavailable scope admission reports `feature.mesh-separate-scope-unparsed`; truncated dependent framing reports `feature.mesh-separate-tail-partial` while readable fields survive.

### MC-06. Legacy mesh combination Boolean selectors

**Question.** Which operations do version-1 mesh combination selectors `0`, `1`, and `2` select?

**Known.** Both scope versions store an operation selector. The decoder applies the version-2 Boolean selector mapping to these version-1 values.

**Need.** Version-1 saves with independently selected join, cut, and intersection operations.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-combine-legacy-boolean-assumed`.

### MC-07. Plane-cut matrix orientation

**Question.** Which matrix convention maps the plane-cut component run to its origin and normal?

**Known.** The scope stores a counted run of sixteen f64 components. The decoder reads row-major translation and the third linear column as the plane normal.

**Need.** Controlled saves with independent translations and rotations around each axis.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-plane-cut-matrix-orientation-assumed`.

### MC-08. Plane-cut translation units

**Question.** Which length unit applies to plane-cut matrix translations?

**Known.** The decoder converts the translation column from centimetres to canonical millimetres.

**Need.** Controlled saves with known translations and different display units.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-plane-cut-units-assumed`.

### MC-09. Plane-cut coordinate space

**Question.** Which coordinate space contains the plane-cut matrix?

**Known.** The decoder interprets the origin and normal in model coordinates.

**Need.** Controlled cuts in translated and rotated nested components.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-plane-cut-model-space-assumed`.

### MC-10. Plane-cut retained side

**Question.** Which control selects the retained half-space of a trim?

**Known.** The scope stores one control byte after the fill selector. The decoder reads zero as the positive retained side and one as the negative retained side.

**Need.** Controlled trims that change Flip while keeping the plane and fill fixed.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-plane-cut-retained-side-assumed`.

### MC-11. Plane-cut face splitting selector

**Question.** Which cut selector splits faces while retaining one mesh body?

**Known.** Selectors zero and one select Trim and Split Body. The decoder reads selector two as Split Faces.

**Need.** A controlled Split Faces save with its cut selector isolated.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-plane-cut-split-faces-assumed`.

### MC-12. Plane-cut uniform fill selector

**Question.** Which fill selector creates uniform closing faces?

**Known.** Fill selectors zero and one select No Fill and Minimal. The decoder reads selector two as Uniform. Fill applies to Trim and Split Body.

**Need.** Controlled saves that select each fill type while keeping the mesh, cut plane and cut type fixed.

**Conflict.** None.

**Note.** The decoder reports `feature.mesh-plane-cut-uniform-fill-assumed`.

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

**Known.** `f3d.md` §3.1 gives the path-pattern settings record. The f64 at offset 94 is 3.0 on a line path whose adjacent spacing is 3 cm and whose start point is at path parameter 0. It is −1.92699 on a circular-arc path of radius 5 cm with a 2 cm spacing and its start point at path parameter π/4. The f64 at offset 124 is 0.5 on that arc path and zero on the line path. The decoder reads neither value.

**Need.** Saves that vary the spacing and the start point on one path, with the two values compared against the instance positions.

**Conflict.** None.

### DR-1030. Circular-pattern spacing control

**Question.** Which bytes store the equal-spacing control of a circular pattern of features? Which bytes store the symmetric control in legacy circular-pattern records whose payload does not read?

**Known.** `f3d.md` §3.1 "A Design circular-pattern scope stores one seed operand group" gives the Compute option and the symmetric byte of the circular payload. The payload of a circular pattern with a 220-byte axis frame does not read, and its distribution controls stay in native data. Their neutral definitions assume equal spacing: a full turn uses total angle divided by count; a partial turn uses total angle divided by count minus one and places the last instance at the total angle. Each circular projection reports `feature.circular-pattern-controls-assumed`, which names equal spacing and the unread spacing control. A partial-turn projection with an unread symmetric byte also reports `feature.circular-pattern-distribution-assumed` for its one-sided distribution.

**Need.** Saves with the equal-spacing control changed separately from the Compute option and the symmetric byte, over three instances on a partial turn with asymmetric seed geometry, with the stored result bodies. Saves of the legacy patterns with the symmetric control changed.

**Note.** The raw legacy distribution controls stay in the native payload.

### DR-818. Targets of rectangular-pattern body references

**Question.** Which body does the entity suffix of the body browser node that a rectangular-pattern body reference names select, and which record does the marked reference after that suffix name?

**Known.** `f3d.md` §3.1 "A rectangular-pattern scope can store an indexed pattern payload" gives the body references of the distribution block. A feature seed stores the references, and each one names a body browser node. The entity suffix names a body that the pattern acts on, and the marked reference after the suffix names the feature scope that made that body. The decoder checks the node type and counts the references.

**Need.** A rectangular pattern of features whose target body is a body that a scope other than the seed's scope made, with its body references, to tell whether the entity suffix selects the target body or the seed's body.

### DR-925. Suppressed-instance keys of a symmetric circular pattern

**Question.** Which instance does each suppressed-instance key name in a symmetric circular pattern?

**Known.** `f3d.md` §3.1 "A Design circular-pattern scope stores one seed operand group" gives the key `k + 1` of instance `k` of a one-sided pattern. In a symmetric pattern of five instances over a partial turn, key `2` names the instance one step from the seed in the negative sense, and key `5` names the instance two steps in the positive sense. The decoder projects a symmetric pattern with suppressed instances with all its instances and charges `feature.circular-pattern-suppression-not-transferred`.

**Need.** Symmetric patterns with other counts, over a partial and a full turn, with suppressed instances on both sides of the seed.

### DR-644. Atomic regions with holes

**Question.** How does a nested profile-region selection store an atomic region that has a hole?

**Known.** `f3d.md` §3.1 gives the atomic-region list of a selected region. Every observed atomic region has one loop. A selected region with a hole and zero atomic regions stores its outer loop with exterior byte `1` and the hole loop with exterior byte `0`.

**Need.** A save that selects two adjacent regions, one of which has a hole.

**Conflict.** None.

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

**Known.** `f3d.md` §3.1 "A u32 group count, that many marked group references" states that in the compact, marked, and counted prefixes the remaining ordered references are the members of the face group. The decoder projects such a `Shell` from its wall slot and face group and reports `feature.shell-table-members-assumed` with the remaining references and the group members.

**Need.** A `Shell` scope whose remaining ordered references differ from its face-group members, with the result body that shows whether those references change the walls.

**Conflict.** None.

**Note.** None.

### DR-600. SurfaceUntrim retained groups and prefix variants

**Question.** How do stored manual-loop groups affect a nonmanual loop choice, which selection does an incomplete group definition denote, and what does a nonnull trailing identifier select?

**Known.** `f3d.md` §3.1 "A `SurfaceUntrim` scope removes trim loops" defines the loop choices and the ordered face and loop-group references. A nonmanual choice can retain loop-group references. The primary prefix closes at the ordered-reference count.

**Need.** Saves that change the loop choice while retaining selected loops, complete definitions for each referenced group, a save with the unpadded closed prefix, and a save with a nonnull trailing identifier.

**Note.** `feature.surface-untrim-stored-loop-groups-assumed` reads stored groups alongside a nonmanual choice as inactive selections. `feature.surface-untrim-loop-groups-aggregated-assumed` represents multiple manual groups by their scope expression. `feature.surface-untrim-group-selection-assumed` retains a scope expression for missing, ambiguous or inconsistent groups. `feature.surface-untrim-padding-assumed` admits uniquely closed unpadded runs. `feature.surface-untrim-frame-assumed` projects the operation with unresolved settings when its prefix has no closed reading. `feature.surface-untrim-trailing-identity-assumed` retains a nonnull identifier as auxiliary metadata while applying the decoded loop and extension settings.

### DR-601. SurfaceUntrim signed extension and missing scalar definitions

**Question.** Does a negative evaluated extension contract the exterior boundary, and which scalar definition applies when a referenced owner is incomplete or duplicated?

**Known.** The extension reference names a numeric owner of local ordinal zero in the same scope. Its evaluated scalar is finite and expressed in centimetres. Extension applies when an exterior boundary is removed.

**Need.** A save with negative extension and its resulting boundary, and complete owner definitions for ambiguous extension references.

**Note.** `feature.surface-untrim-negative-extension-assumed` retains a negative scalar as signed neutral extension. `feature.surface-untrim-extension-unresolved` preserves the parsed frame when its scalar owner does not resolve uniquely. `feature.surface-untrim-extension-unrepresentable` retains the native value when its millimetre conversion overflows.

### DR-625. Counted Shell prefix byte and partial envelopes

**Question.** Which option does the counted Shell prefix byte select, and which fields preserve the wall-slot direction when that byte is nonzero or the envelope tail is incomplete?

**Known.** The counted prefix has a byte between its fixed marked prefix and its aligned optional wall references. The references can join finite positive wall scalars independently of the prefix byte. The scalar owner and its source location remain available. The zero-byte form has inside-first slots.

**Need.** Controlled saves varying that byte and the inside and outside walls separately, with complete prefix, auxiliary-reference, group and GUID runs.

**Conflict.** None.

**Note.** Partial Shell projection reports `feature.shell-counted-control-assumed` for reading the byte as a separate prefix field, `feature.shell-slot-direction-assumed` for the nonzero form's inside-first reading and `feature.shell-envelope-assumed` when a unique partial prefix supplies wall data. Unresolved values and selections remain in `shell:source` with their references and byte locations.

### DR-626. Simultaneous Shell face and body groups

**Question.** How do the removed-face group and the target-body group jointly select a Shell operation when its prefix names both groups?

**Known.** A cornered prefix can name both a removed-face group with role `0x0000001000000000` and a body group with role `0x0000000400000000`. Each group retains its own members. The wall scalar and both cornered prefix Booleans remain independently readable.

**Need.** Controlled saves changing the face selection and body selection separately, with the corresponding input and output bodies.

**Conflict.** None.

**Note.** Partial Shell projection retains both selections and reports `feature.shell-group-roles-assumed`. The wall-selection solver uses the admitted complete construction; partial wall assumptions remain separate from its geometry proof.

### DR-311. Trailing reference of a surface boundary carrier and other SurfaceExtend prologues

**Question.** Which records can the trailing reference of a `SurfaceExtend` or boundary-carrier `SurfaceOffset` group name, what does the boolean of a compact flag record state, and where does a `SurfaceExtend` scope with another prologue store its extension method?

**Known.** `f3d.md` §3.1 "A `SurfaceExtend` scope has at least three ordered references" defines the distance owner, the boundary carrier, and the scope prologue with the extension method. The perpendicular extension in DR-312 has a shorter prologue: eighteen zero bytes follow its indexed header, then the marked distance-owner reference. The boundary carrier is a counted construction-operand group with role `0x0000000800000000`, `trailing_count` 1, and one trailing reference. A compact flag record has an indexed header followed by ten zero bytes, marker byte `01`, a boolean byte, and terminator byte `00`. The boolean is `1` for each extension method with edge chaining on.

**Need.** Two saves of one extension that differ only in the edge-chaining option, compared byte for byte. Controlled saves and record comparisons that establish the trailing reference target forms and their effect on the operation. Controlled saves with the shorter prologue and different extension methods.

**Conflict.** None.

**Note.** The decoder reads the extension method from the fixed scope prologue. With a positive distance, a scope with another prologue or a method code above `2` projects with `SurfaceExtension::Unresolved` and reports `feature.surface-extend-method-unresolved`. The decoder selects input-state faces for the boundary when each boundary edge has one edge-recipe operand, all operands name one input state, and each operand has one candidate face in that state with a boundary edge that the feature changes. A feature that keeps this selection reports `feature.surface-extend-edges-as-faces` and stays in `feature.definition-incomplete`, because the neutral operation selects faces and the edge choice stays native. Another boundary keeps its native reference. The decoder retains each boundary operation and reports `feature.surface-boundary-flag-record-assumed` when its trailing reference does not name a compact flag record.

### DR-312. Negative SurfaceExtend distance

**Question.** Which saved fields distinguish a completed negative `SurfaceExtend` operation from a failed extension?

**Known.** The distance owner's evaluated scalar is a finite signed value in centimetres. The linked parameter's expression states the same signed value, for example `-0.2 mm`. The neutral `ExtendSurface` distance is positive. A natural extension with a negative distance is a failed feature in Fusion: the scope keeps its frame, its signed distance and a history state, and the body keeps its unextended boundary. A shorter-prologue scope stores `-3 mm`. Its ASM transition adds a planar face perpendicular to the source patch. The new face extends 3 mm along the negative normal of the patch.

**Need.** Paired saves of one perpendicular extension and one tangent extension at a positive distance and its negation, with the feature health and result body of each save. Compare the scope and ASM transition of each pair.

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

**Note.** A scope without a decoded radius carrier projects as a neutral Fillet with an unresolved radius law and dimension. `feature.fillet-radius-unresolved` identifies that uncertainty; `feature.fillet-references-unresolved` retains the unclassified reference contribution. Decoded source records and ordered references remain in `fillet:source` and the scope reference properties. A full-round projection requires the documented group structure and a matching bounded-face operand; a native or partial center selection carries `feature.fillet-full-round-center-unresolved`. A missing bounded-face definition does not establish full-round semantics.

### DR-322. Selected-face Extrude start without a start group

**Question.** What does start-support byte `2` mean for an `Extrude` scope that stores no start-face group?

**Known.** `f3d.md` §3.1 gives start-support byte `2` as a selected-face start that stores a `ProfileOffset` and a profile-from-entity reference. A selected-face start adds one role-`0x0000001100000000` start group; a role-`0x0000000500000000` group is a start-face group only when the start support is selected face. A new-body `Extrude` scope exists with start-support byte `2`, one-sided distance extent, and a `ProfileOffset` owner, whose reference table holds three fixed scalar lanes, one further record, the profile group, and its profile. It stores no start group of any role. Its reference slots and frame length are those of an offset-profile-plane new-body scope with the same reference count. Selected-face start scopes of other operations store the start group. A one-sided to-face `Extrude` scope with start-support byte `2` stores one role-`0x0000001100000000` face group, which its seventh reference slot names, and a `ProfileOffset` reference in its first slot. When the seventh reference slot names the single face group, the decoder reads that group as the termination and the start as the offset profile plane, and reports `feature.extrude-start-face-absent-assumed`. A single face group that the seventh slot does not name is the start group.

**Need.** Saves of a new-body `Extrude` that start from a selected face, from an offset plane, and from a selected face that is later deleted, each with the start-support byte and the scope reference table recorded.

**Conflict.** None.

**Note.** A one-sided distance New Body scope with a `ProfileOffset` and no face or shape target group projects with an unresolved start face. The neutral feature retains the signed start offset and blind distance. The decoder reports `feature.extrude-start-face-unresolved`.

### DR-326. Extrude start face on another component's body

**Question.** Which identity in a role-`0x0000001100000000` start group names the selected face when that face belongs to a body of another component, and in which ASM history and state is it resolved?

**Known.** The group uses the 143-byte direct construction-group frame of `f3d.md` §3.1. Its sole member is a bounded-face recipe operand, and its two auxiliary records each carry one u64 entity identity. The recipe's references resolve no face in the input state of the Extrude's own history. The two auxiliary identities also occur in a second history, as a surface and a pcurve. The surface carries one planar face, and the coedge of the pcurve lies on a different face. In the Extrude's own history, both identities occur as coedges, first inserted after the Extrude's state. Each auxiliary identity occurs in both histories. No single history holds both identities on one face. Both auxiliary records use the compact placement form with variant byte `1`; neither carries a rigid transform. The decoder keeps the start face native.

The group also uses a 144-byte counted frame with one bounded-face member. Recipes of 12, 14, and 72 prefix references occur in this form; every prefix reference has an empty face lane in the preceding state that the owning scope names. Several start groups of one design can share an auxiliary identity: three groups store the ordered pairs `(a, b)`, `(c, a)`, and `(d, b)`. Each auxiliary path has compact placement with variant `1`.

The Design type table registers each auxiliary identity as an entity of Fusion type `549FBB80-B890-473E-A5C0-415D3D9BF4E6`, version `4`. The 72-byte component-context record in `f3d.md` §3.1, "Records the timeline context names," has this type and version.

The bounded-face programs of these groups have 13, 23, and 12 nodes. The first two programs each have five one-clause nodes. The third program has twelve two-clause nodes. DR-335 covers the one-clause binding.

**Need.** The join from the auxiliary Design records to the source component and its selected ASM state. Designs with two components, A placed once in B. B's first `Extrude` starts from a planar face of a body in A and extends a sketch profile 5 mm. Save one design per selected face of A: the top face, a side face, and a face that a later feature of A changes. Save a further design where A is placed twice and the face of one occurrence is selected. Each save needs the start group frame, its auxiliary records, the ASM histories of both components, and the occurrence transforms of A in B.

**Conflict.** `f3d.md` §3.1 names the sole member of a 143-byte direct construction-group frame a persistent entity-selection operand. In this group it is a bounded-face recipe operand, and the decoder reads it as a face operand.

**Note.** None.

### DR-327. Separator byte and wrapper identity of a 130-byte circular-pattern axis record

**Question.** Does the `u8 1` that a 130-byte two-wrapper circular-pattern axis record stores between its references to the axis-record indexes plus two and plus one encode the direction of travel? Which topology does the persistent identity of its wrappers name?

**Known.** The 129-byte two-wrapper form stores two zero bytes there, and the 130-byte form stores `u8 1` and two zero bytes, with every later field one byte later. The 220-byte rectangular-pattern direction record stores the same `u8 1` and two zero bytes between the same two references. The decoder does not yet resolve the historical axis edge of the 130-byte form, so a pattern whose axis comes from that form keeps its native transform. A full turn would project once the edge resolves, because its instance set is independent of travel direction. Both wrappers of one 130-byte record carry one persistent identity. The Design type table lists that identity under the body-visibility browser-node member type `D26351F0-5940-4D23-AA20-2C35475A6D9E`. The wrapper identity stays native. A 220-byte inline carrier of the same pattern supplies the pattern axis independently.

**Need.** Saves of one circular pattern of a body about an edge axis, with a total angle of 90 degrees and three instances, once in each direction of travel, plus the same pattern as a full turn. Save the full-turn pattern once about a linear edge, once about a circular edge, and once about a construction axis. Each save needs the axis record frame, its wrapper records, the parameter owners, and the ASM history. A resolution of the wrappers' persistent identity to the historical axis edge would supply the resolved axis that the circular-pattern projection takes for this form.

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

**Known.** An edge operand's candidate faces are the faces of the bound ASM history whose persistent subentity tags name the recipe's prefix record index. An edge operand can store record index `1`. The bound history then holds no persistent subentity tag that names design reference `1`, so the operand has no candidate faces. Such an operand stores either no prefix references or prefix references that resolve to between 4 and 44 faces. An operand with another record index can have candidate faces, but no fixed relation joins those faces to the faces of the prefix references: the candidate faces can be equal to neither the union of the reference faces nor the union of the faces tagged by the positive references, and not a subset of the reference faces. The decoder keeps the edges of an operand with record index `1` native.

**Need.** A single-body part whose `Fillet` rounds one edge of a box, saved once. Then the same `Fillet` with a second edge added, and the same `Fillet` after an earlier feature that changes one face next to the edge. Save each design in the top-level component and once in a component placed in another design. Each save needs the edge-recipe operand records, the parameter scope, and the ASM history.

**Conflict.** None.

**Note.** None.

### DR-336. Placement-owner As-built scope with an 87-byte tail

**Question.** How does a placement-owner As-built scope with an 87-byte tail bind its connector frames, motion, and limits?

**Known.** A placement-owner `As-built` scope with an 87-byte tail has class pair 371/263, type `AA8B3D40-B67D-4AF1-B3CF-E1E00279B5F0` version 1, a 421-byte frame, and an eleven-entry reference table. The owners at positions five through eight supply three zero offsets and a zero angle. The owners at positions nine and ten supply −π/6 and zero radians. The prefix names two locators, each of type `2E3AC990-483D-4A55-8662-C064B976A957` version 3. Both locators store a null construction reference and a matrix. The entry at position zero is a construction of type `7B6C3C2D-4096-4E6D-B225-699E8C2D9355` version 8 that stores a decoded placement matrix. A motion record references two occurrence records and the joint record `J` whose next record `J + 1` is the scope. It stores freedom code 2. The motion decoder binds it to the occurrence record that its reference at offset 24 names. With the decoded placements of the two occurrences, both locator matrices compose to the same frame in the occurrences' shared parent within a maximum matrix-cell difference of 2.5e-14 after translation-unit conversion.

**Need.** Define the roles of the motion record's ordered references and the admission rule that distinguishes the placement-owner layout from the direct-construction layout. Establish the motion and limit association for this layout.

**Conflict.** The direct-construction 421-byte layout places scalar owners at reference positions four through seven and its solved frame at position eight. This scope places scalar owners at positions five through eight and stores a selection record at position four. Frame length selects two distinct layouts.

**Note.** The decoder retains the scope and its six parameter values. Neutral projection requires the placement-owner layout and its motion binding together.

### DR-338. Extended circular-pattern axis controls and selection runs

**Question.** Which entities do extended circular-pattern axis selection runs select, and how do the 216-, 220-, and 241-byte forms determine their directed axes?

**Known.** The decoder admits the 195-byte inline axis frame, the 220- and 221-byte frames with selection kind `8` as inline axes, and the 118-, 129-, and 130-byte identity forms. It retains the 195-byte and 221-byte precedence and uses a 220-byte frame when those forms are absent. A 220-byte frame with control `7`, origin `(0, 0.175, 0)` cm and displacement `(0, 2.075, 0)` cm occurs in a pattern of count 20 over one turn and in patterns of count 2 over a quarter turn. The two bodies of such a quarter-turn pattern have centroids approximately `(4.3174141013582314, 14.75, 0)` mm and `(0, 14.75, -4.3174141013582314)` mm, a positive quarter turn about the stored axis. An extended frame carries an origin and displacement in the same positions as the rectangular-pattern direction frame. Three body instances over a quarter turn about a 221-byte frame with a Z displacement lie at 0, 45, and 90 degrees counterclockwise about that displacement. A 221-byte component-pattern frame can carry a unit X displacement and an origin at Z = 1.92 cm. The saved occurrence transforms of its pattern are positive quarter turns about that line. A 220-byte feature-pattern frame carries a Z displacement. Its seed holes occur in symmetric pairs, so positive and negative quarter turns yield the same saved hole set. A 241-byte body-pattern frame carries an X displacement and three consecutive selection references. Its total-angle owner stores a full turn. A 216-byte frame stores control `7` at offset 89 and three consecutive marked references at offsets 97, 108, and 119. The count of its pattern is two and the total angle is a quarter turn.

**Need.** Bind each extended selection run to its selected entity. Compare the controls of the 216-, 220-, and 241-byte forms for three instances over a partial turn in each travel direction, with asymmetric seeds. Bind each instance to its saved BREP geometry.

**Conflict.** None.

**Note.** DR-327 tracks the separator of the earlier two-wrapper form.

### DR-339. Circular component-pattern seed paths

**Question.** How does a circular pattern's role `0x0000010000000000` path group identify its seed occurrence and ordered generated occurrences?

**Known.** A circular component-pattern scope can have an empty member run in this role and two auxiliary paths. Such a scope has one later ordinal-2 occurrence carrier and an earlier ordinal-1 carrier with the same component GUID. The generated carrier stores the positive quarter-turn transform described in DR-338. The decoder's component-pattern occurrence binder consumes rectangular-pattern instance frames. Circular-pattern seed selection requires a nonempty face or body group.

**Need.** Join the auxiliary path identities to occurrence carriers through stored references. Confirm the join with multiple earlier occurrences of the same component and with reordered generated occurrences. Bind the selected occurrences to saved instance geometry.

**Conflict.** None.

**Note.** None.

### DR-341. Chamfer distance reference face

**Question.** How do the set controls and ordered recipe references assign the first distance of a two-distance chamfer, or the distance of a distance-angle chamfer, to an input support face?

**Known.** Chamfer sets reference their edge groups and dimensional owners independently. One scope can combine equal-distance, two-distance, and distance-angle sets. Chamfer parameter owners supply ordered dimensional specifications. Edge recipes retain ordered sides and prefix-reference ordinals. Exact ASM transitions retain support-face carriers and the inserted treatment carriers. The projector transfers the dimensional specification and the edge selection into `ChamferGroup`. That IR type carries the selection and specification. A two-distance or distance-angle set stores a distance-reversal byte (`0` or `1`) at primary-relative offset 22. With `0`, the first distance of a two-distance set, or the distance of a distance-angle set, is the setback along the face whose coedge runs in the direction of the treated edge in the state before the feature; with `1`, the two faces exchange roles. The projector sets `flip_direction` to `true` when any resolved asymmetric set has its reversal byte set and `false` otherwise. On the top edges of a cube, two distance-angle sets store 1 mm and 30 degrees with reversal bytes 0 and 1. Their selected edges are at `y=10, z=10` and `y=0, z=10`, and their result treatment planes give top/side setbacks of `(1, 0.577350269189626)` mm and `(0.577350269189625, 1)` mm. Two two-distance sets store first/second distances `(1, 3)` mm and `(3, 1)` mm on the edges at `x=10, z=10` and `x=0, z=10`; their top/side setbacks agree with that order. A single chamfer on a front top edge with first/second distances `(1, 3)` mm or `(3, 1)` mm has top/front setbacks equal to those distances. With 1 mm and 30 degrees on that edge, reversal bytes 0 and 1 give top/front setbacks of `(1, 0.577350269189626)` mm and `(0.577350269189625, 1)` mm. Each 30-degree treatment plane forms that angle with the face carrying the 1 mm setback.

**Need.** A rule that binds each selected input edge to its distance reference face, with both face widths checked against the stored dimensional specification. The rule must cover reversed recipe-reference order, propagated edge chains, and support faces split by earlier operations.

**Conflict.** None.

**Note.** Projection retains each distance-reversal flag in its native set, sets `flip_direction` when any resolved asymmetric set is reversed, and reports `feature.chamfer-reference-face-assumed` (including when asymmetric sets within one scope disagree on reversal). For an unequal two-distance chamfer whose selected edge meets two planar or plane-and-cylinder faces with one forward coedge, input-edge support records store both faces and their outward normals with the forward-coedge face marked `with_edge` as the assumed Distance 1 side under `feature.chamfer-reference-face-assumed`. The neutral representation still needs a proven reference-face binding per selected edge to retire the assumption.

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

**Note.** Projection reports `feature.chamfer-angle-reference-assumed` when it reads the stored angle as the neutral reference-face angle. DR-341 tracks the input reference-face assignment.

### DR-640. Chamfer parameter recovery from incomplete definitions

**Question.** Which value and role remain authoritative when a Chamfer set's parameter definition, owner backlink, source kind, or unit token is incomplete or inconsistent, and when can an owner's evaluated scalar be stale?

**Known.** A set's kind word identifies its distance and angle slots. The set references their owners. A parameter owner carries an evaluated scalar and a parameter reference; the parameter carries an owner backlink, source kind, and optional unit token. Fixed scalar records retain their record identities and values independently of a set list.

**Need.** Controlled saves that distinguish current parameter values from stale owner values, parameter namespaces that share an owner, and incomplete reference graphs with independent dimensions for each set.

**Conflict.** None.

**Note.** The decoder uses a unique parameter backlink when its owner is absent, with `feature.chamfer-parameter-owner-assumed`. It uses a stored owner or fixed scalar when the parameter definition is absent, with `feature.chamfer-parameter-value-assumed`. Explicit slots select the most likely role and canonical unit, with `feature.chamfer-parameter-kind-assumed` or `feature.chamfer-parameter-unit-assumed` for a discrepancy. Ambiguous definitions retain the identified form and raw parameters with `feature.chamfer-definition-unresolved`. Explicit set references survive an owner-order conflict with `feature.chamfer-set-conflict`.

### DR-360. One-sided face-target Extrude without a termination group

**Question.** Which stored relation supplies the termination face of a one-sided face-target Extrude whose operand roster has no termination group?

**Known.** A one-sided face target (direction `1`, first-side extent `2`) stores one role-`0x0000001100000000` termination group, distinct from its start group. A selected-face start adds one start group before it in scope-reference order. A shape target replaces the termination group with one role-`0x0000000500000000` target group.

**Need.** The termination face of such a scope, or the stored state that makes the extent end without one.

**Conflict.** A one-sided Cut scope can store a selected-face start, one start face group, a `Side1Offset` owner, no termination group, and no shape-target group.

**Note.** The Conflict scope has the DR-322 form: its seventh reference slot names its single face group.

### DR-361. Treated edges in executed treatment transitions

**Question.** Does an executed `Fillet` or `Chamfer` transition delete every treated edge?

**Known.** The two-face group rule assumes that the transition deletes every treated edge and that the treated edge set equals the transition-deleted set. The rule checks that the deleted-set size equals the member count, each member has two distinct preceding faces, each member's shared changed-edge candidates are nonempty and belong to the deleted set, the candidates cover that set, a perfect assignment pairs each member with a distinct candidate, and each resolved member edge belongs to the deleted set. A group selected by this rule charges `history.edge-selection-deleted-set-assumed`.

**Need.** A treatment whose treated edge survives the transition, or proof that every treated edge is deleted.

**Conflict.** None.

**Note.** None.

### DR-376. Stored name of a Sketch renamed in the timeline

**Question.** Where does a Sketch renamed in the timeline store its name?

**Known.** The placement reader in `design/decode/sketch.rs` reads the display-name slot in its empty form. The stored Sketch placements hold the slot in its empty form. The projected Feature name joins the stored scope kind spelling, a space, and the feature ordinal. A planar or spatial Sketch takes this name when one placement in the same stream carries its scope, one Feature with a non-empty name projects from that scope, and one scope selects the sketch.

**Need.** A design with a Sketch renamed in the timeline.

**Conflict.** None.

**Note.** The decoder charges one aggregate `sketch.name-composed-from-feature-ordinal` loss when sketches take the composed name.

### DR-340. Compact profile loop groups and their region descendants

**Question.** Which regions of a compact profile loop group form the feature selection when the group carries an initial region and further regions that divide it?

**Known.** An Extrude profile operand `N` names a Sketch entity. Its nested record `N+3` starts with a reference to `N` and a counted loop recipe (`f3d.md` §3.1 "A sketch-profile record `N` can carry a nested profile-region selection"). The first region of such a group can be a rectangular annulus: four exterior entries and four hole entries, each hole entry with a nonzero secondary identity. Two further regions divide this annulus at a rectangle of four further curves. Each entry has kind 3, a primary and secondary identity, three operands, and eight zero bytes. Every listed identity binds to a curve owned by the Sketch. The neutral Sketch carries two profile loops: the inner rectangle, whose primary identities are those of the hole entries, and a central hexagon. The curves of the outer rectangles stay outside those two loops. The decoder retains the compact loop groups in `design_profile_curve_selections`. Its complete-loop projection requires an exact loop join and a complete analytic Cut-transition certificate. A construction group of the scope owns the profile operand. A persistent selection group also belongs to the scope; its member carries a local identity. The direct compact curve join requires an empty further-region list. A complete initial boundary can identify a union across subdivisions through directed interval equality, analytic output caps, and bounded lateral faces. A transition between two consecutive history states inserts four lateral faces and updates three parallel cap faces. The two rectangle diagonals of the Sketch use the construction-tail frame tracked in DR-463.

**Need.** The stored discriminator for different proper subsets of the descendants. Bind the three entry operands to direction and curve portions for the compact spelling.

**Conflict.** None.

**Note.** The Extrude retains its native selection group. Existing `SketchRegions` with explicit boundary uses can represent the rectangular annulus after its selected boundaries and traversal directions are established. The nested profile-region selection in `f3d.md` §3.1 stores the same structure: each selected region is followed by its atomic regions, the arrangement faces that together make it. In saves that select one, two adjacent, and two separate regions of a rectangle split by a line, every atomic region of a split selected region is listed, and a selected region that is one face lists zero atomic regions. The compact loop group can follow the same rule.

### DR-358. Compact complete-loop region identity across sketch arrangements

**Question.** Which stored relation selects an atomic region for a compact complete-loop group when a Join updates its far end cap, has another start-cap outline, has several bounded member regions, a two-sided Join exposes partial end sections, or the feature has an empty face delta, an unbound history state, or a selected-face start?

**Known.** The compact group lists exterior and hole loops through persistent curve identities. An exact neutral loop join identifies each boundary. A one-sided blind Cut from the profile plane can certify its candidate set through the changed ASM planar regions and analytic lateral carriers. The decoder retains the native selection when this certificate is incomplete. An inserted analytic end-cap union can certify a complete initial boundary for a one-sided blind Join or NewBody when its directed intervals agree and every inserted lateral face stays on the boundary within the sweep or continues a deleted lateral face on the same carrier. A two-sided blind Join or NewBody can certify its selection through equal gained region sets at both signed end heights, exact changed parallel sections, and bounded inserted, updated, and deleted lateral faces. An updated rectangular start cap aligned with the Sketch axes certifies a single exterior line group when its member graph has one bounded region and the complete changed-face envelope agrees with the rectangular sweep. A scope can have an updated far end cap, an empty face delta, or an unbound history state.

**Need.** The relation between the compact entry operands, the arrangement region identity, and the selected side of each boundary for these transition forms, including intersecting and nested loops.

**Conflict.** None.

**Note.** DR-340 covers the further-region program. This item covers groups whose further-region counts are zero.

### DR-335. Extrude bounded-face programs across boundary changes

**Question.** Which stored relations bind an `Extrude` start or termination recipe when its node run covers an earlier boundary or a prefix operand has an empty face lane in the preceding topology?

**Known.** A two-sided termination stores two bounded-face recipes with the same Design reference. One program has five nodes, of which four have a parsed two-clause structure. The other has four parsed two-clause nodes; its first prefix operand names a face slot whose preceding loop has eight edges. Its aggregate candidate lane contains four side faces. A further termination program has eight parsed nodes and two empty prefix-face lanes. Two further programs have five nodes each, including a node with one side clause, which binds a closed boundary edge (`f3d.md` §3.1). Their remaining candidates include several preceding faces.

A start group with one bounded-face member can have a recipe of two one-clause nodes and three prefix operands. The middle operand names a face through a positive Design reference and token `6`. The first and third operands have negative Design references and empty face lanes. The selected-face candidate and historical support lanes are empty.

A start group with one bounded-face member and a 123-byte primary frame has zero auxiliary path records. Each row below gives the counts of one such member, each counted in the preceding state of its scope.

| Nodes | One-clause nodes | Preceding candidates | Historical support contexts |
| --- | --- | --- | --- |
| 7 | 1 | 12 | 15 |
| 20 | 1 | 152 | 152 |
| 9 | 1 | 104 | 104 |
| 13 | 1 | 43 | 43 |
| 33 | 2 | 2 | 2 |
| 62 | 5 | 0 | 0 |

The 13-node member has one empty prefix-face lane. The 62-node member has 63 prefix operands, each with a nonempty face lane, and its selected-face candidate lane is empty.

**Need.** The mapping of a saved boundary edge to a chain of preceding edges, and the binding of empty prefix-face lanes (DR-170). Independent selections before and after a boundary split must distinguish a selected face from its adjacent faces.

**Conflict.** None.

**Note.** These start and termination groups retain their native identities when the candidate, support, complete-loop, and plane proofs remain unresolved.

A compact Sketch-point termination of an Extrude is tracked in DR-480.

### DR-359. Region selections with curve identities absent from their Sketch

**Question.** When a counted region selection names a curve identity that its Sketch no longer owns, do the atomic regions that the stored Cut transition removes equal the selected regions?

**Known.** A later Sketch edit can remove a curve identity that a region selection names. The reading takes the atomic arrangement regions that lose material on a changed plane parallel to the Sketch. The candidate set must pass the compact Cut certificate, its region count must equal the stored region count, and every stored member identity that the Sketch still owns must bound the corresponding region. A feature read this way charges `feature.profile-region-identities-stale`.

**Need.** The stored relation between a removed curve identity and its replacement curves, or a Cut whose stored transition removes a region outside its selection.

**Conflict.** None.

**Note.** DR-358 covers compact selections whose curve identities all resolve.

### DR-357. Compact owned Thread option semantics

**Question.** What property does the prefix word encode, and what construction option does the byte `C` encode beyond its owner-reference order?

**Known.** The version-four class-`326/263` compact owned frame has a zero u32 at primary-relative offset 33. The compact marker starts at offset 37, the `6H` token at offset 42, and the designation at offset 46. Construction marker `00 01 00 00 00` has leading offset and length owners; marker `00 02 00 00 00` has trailing offset and length owners. Both trailers repeat the length and offset owner references. The selected internal cylinders have the stored minor diameters. The decoder retains these forms and their owner order.

**Need.** Controlled changes to the prefix word and construction option, with complete scope bytes, face orientation, and resulting geometry.

**Conflict.** None.

**Note.** DR-142 covers construction options. DR-205 covers modeled extended-prefix entries.

### DR-334. Persistent tag token integer meaning

**Question.** What does the u32 integer after a Design recipe-prefix token and between a B-rep generic-tag token and its reference count mean?

**Known.** The pair (token, token integer) identifies the persistent tag group. The decoder retains the integer in generic-tag groups and both standard-prefix token frames. A Design reference joins a tag with the same pair and Design reference. Most groups store integer `0`. Edge-recipe prefix fields can store a nonzero integer, with values such as `4012`, `6798`, and `12173` after tokens `3273`, `726`, and `3666`. A nonzero recipe field can have a live generic-tag group with the same pair. The compared body-map suffixes, history-state identities, reference counts, and byte lengths differ from the integer. Edge-recipe occurrences of `4012` can equal an indexed record index and a Design type-table entity ID of the same Design stream, or differ from both. An Extrude prefix can store `(5, 82902527, -300)` and `(5, 1773368033, -300)`, and an input face of that Extrude carries the same groups. The latter operand is unframed. A later Extrude prefix of the same Design stream stores token `10` with integers `1437049394` and `1888070796`, each with references `-300`, `328`, `329`, `-372`, and `-373`; one input face carries both groups. These complete prefixes resolve to those two input faces. Main selected a different cylindrical face for each prefix after rejecting the prefixes and the nonzero tag groups. The intermediate reader retained the tag groups and rejected the unframed nonzero prefix operands, leaving both selections native. For token `10` with references `328` and `329`, equal-integer matching selects the input face that carries both groups; token/reference matching adds four faces whose groups store integer zero. All other references in the two prefixes have the same live candidates under both joins. The disjoint counted-name operands in DR-462 use integer zero in every name.

**Need.** Determine whether equality of the retained integer is an identity invariant across revisions and how to frame an unframed token when several token lengths give valid integer/count/reference fields. Compare paired saves with the same persistent edge selection before and after an operation that gives a tag group a nonzero integer, for example a Fillet on an edge of a body that a Combine or a Split later changes. Each save needs the recipe prefix and the generic-tag groups of the BREP history. Resolve the counted-name candidate composition in DR-462 independently of integer equality.

**Conflict.** None.

**Note.** Zero and nonzero integers use the same pair comparison.


### DR-462. Counted persistent-name candidate composition

**Question.** How do the persistent names of one counted recipe operand constrain the selected face fragments when their live candidate sets overlap or are disjoint?

**Known.** Fillet and Chamfer members can carry counted operands with several persistent names. Intersecting the live name sets of a 29-member Fillet and a 12-member Chamfer resolves one edge per member; the transition of each feature deletes every resolved edge. Name union admits extra face fragments through `(2, 0, -420)`, `(2, 0, -421)`, `(1, 0, 328)`, and `(1, 0, 329)`, which makes singleton edge intersections ambiguous. A Fillet member can have a counted operand with `(1, 0, 530)` on two faces and `(7, 0, 375)` on a third face. Its other primary-side operand names a fourth face through `(6, 0, 375)`. The first operand's name sets are disjoint. Their union and the other operand share the edge between the third and fourth faces; the Fillet transition deletes that edge. A Chamfer transition can preserve main's selected edge and delete all fourteen edges selected with the common-name constraint. A Chamfer can have ten counted operands whose names each carry two Design references, one of which is common to six fragments of token `1`. The faces that carry every reference of each name select the ten edges of one face loop; the Chamfer transition deletes all ten and adds ten faces. The union of the references selected four edges outside that loop, two of which the transition keeps.

**Need.** Determine the relation between name order, historical name validity, and face fragmentation. Compare paired saves with the same selected edge before and after a topology change that makes the live name sets disjoint. Retain the counted prefix, the input and result generic-tag groups, and the complete edge transition.

**Conflict.** The common-name constraint removes aliases in the first case of the Known and a transition-deleted edge in the second. A Fillet member can name token `6` with two consecutive Design references. The faces common to both references select no edge; the union selects an edge on a face whose tag carries only the first reference, and the Fillet transition deletes that edge. Two further members show the same pattern, each for one edge.

**Note.** Within one name of a Fillet or Chamfer edge recipe whose first side names at least two reference ordinals, the decoder first uses the faces common to all Design references. It uses their union when no face is common or the common faces select no transition-deleted edge. The decoder uses the nonempty name intersection. For disjoint live name sets it retains their union and requires a unique transition-deleted common boundary edge for the fragment fallback. The inferred-selection loss records this assumption.

### DR-362. Non-outer loops in Loft section selections

**Question.** What do the non-outer loops of a Loft section's stored region selection select?

**Known.** A Loft scope can have two section groups on two Sketches. The first section group lists its Sketch's six-curve outer loop and its nested ten-curve non-outer loop. The second section group lists outer loop A and nested non-outer loop B of its Sketch. The stored body after the Loft has one lofted side face and two filled planar caps. Each cap has one loop. The lower cap follows the complete outer loop A of the second Sketch; the upper cap follows the six-curve outer loop of the first Sketch. Native section groups and counted spatial-profile member lists remain selection-coverage limits.

**Need.** A Loft whose stored body has a hole in a section cap, or a Loft whose stored section lists only an outer loop while that loop contains another loop of the same Sketch. Compare the stored selection flags with the cap topology.

**Conflict.** The Extrude reading of the same record form uses the exterior flag to select the exterior loop and the other loops to select holes. That reading selects rings for both sections of that Loft, while the stored caps have filled areas.

**Note.** The Loft section binder retains selections with non-outer loops as native profiles and records `feature.profile-selection-native`.

### DR-461. One-sided face-target Extrude direction and offset reference

**Question.** Which vector defines the signed `Side1Offset` of a one-sided face-target Extrude, and how is travel selected when the target lies behind the start plane?

**Known.** An Extrude record can store profile normal `+Z`, profile offset `0.75` cm, direction `1`, direction reversal `0`, and `Side1Offset = -0.15` cm. Its ASM transition inserts a plane face at z=7.5 mm, a plane face at z=18.5 mm, and 120 cylindrical side faces with boundary z coordinates from 7.5 to 18.5 mm. The input state has planar annular faces at z=0 and z=20 mm. The termination operand carries both faces among its recipe candidates. The decoder projects the z=20 mm face as the historical termination: the z=20 mm target with a displacement of -1.5 mm along `+Z` gives the inserted z=18.5 mm cap. Another Extrude record stores profile normal `+Z`, a target face at z=8 mm, and `Side1Offset = 1.6` cm; its transition inserts a plane face at z=24 mm. The travel, profile normal, and target surface normal are parallel in that case.

**Need.** Independent transitions with a nonzero offset whose profile normal, oriented travel, and target surface normal distinguish the possible sign rules. A target behind the start plane must also distinguish the direction-reversal setting from travel toward the target.

**Conflict.** None.

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

**Note.** An unresolved construction prefix keeps a native component selection inside a neutral component insertion and reports `feature.derived-instance-component-insert-assumed`. A decoded prefix whose source relation or occurrence is missing keeps its known selection members and reports `feature.derived-instance-sources-unresolved`. Its native scope reference and ordered reference properties remain present.

### DR-466. Standalone joint source operand binding

**Question.** Which document identity namespace and occurrence context do qualified source paths select when their keys do not match admitted occurrence carriers?

**Known.** The qualified assembly scope carries connector frames and alignment values. Its occurrence path carries the source document identity, occurrence key, and ordered descendant keys. These fields describe a readable joint independently of availability of the referenced document.

**Need.** Document bundles with independently identified source documents and repeated component instances that bind each source path identity to its document and occurrence context.

**Note.** A standalone timeline feature names its decoded neutral joint while source operands remain unresolved. The decoder preserves their identities and charges `assembly.joint-source-binding-assumed`.

### DR-486. DerivedInstance source placement authority

**Question.** Which placement has authority when a derived source matrix differs from its joined occurrence carrier, or the source placement run omits that relation?

**Known.** The source placement table stores marked relation references followed by an identity selector or sixteen finite scalars. A joined occurrence carrier independently stores its placement. Both representations remain in the native construction.

**Need.** Paired saves that independently change the derived source placement and its occurrence carrier, including an omitted source placement.

**Conflict.** Source and carrier matrices can differ.

**Note.** A unique affine source placement supplies the neutral occurrence pose. Disagreement reports `feature.derived-instance-scope-placement-assumed`. An absent, conflicting or unusable source placement uses the carrier pose and reports `feature.derived-instance-carrier-placement-assumed`. A placement that exceeds finite millimetre coordinates remains native and reports `feature.derived-instance-placement-unrepresentable`. An independently bound finite current pose can supply its occurrence; unresolved occurrence membership remains in the neutral insertion.

### DR-487. DerivedInstance context transform and auxiliary fields

**Question.** What are the roles of the prologue reference, the transform after the primary relation and the trailing GUID, and how do the auxiliary operand-path locators qualify the derivation source?

**Known.** The counted prefix has an ordered source placement run, an auxiliary pair run and an independently ordered repeated run of the pairs' relation references. A primary relation follows four zero bytes. A one-byte marker, an independent identity selector or sixteen-scalar matrix, an LP-UTF16 GUID and three zero bytes close the prefix. Auxiliary targets occupy eight bytes and can exceed a 32-bit record index. Typed references retain their inline GUIDs; typed auxiliary references identify operand-path locators.

**Need.** Paired saves that vary the derivation context and each auxiliary target independently.

**Conflict.** None.

**Note.** All fields remain native. The second matrix is not composed into the neutral source occurrence pose; an explicit matrix reports `feature.derived-instance-context-transform-assumed`.

### DR-620. Partial Sweep sections and controls

**Question.** What assigns section semantics to the role-`0x43` group, and what distinguishes a profile carrier from its separate selection group? Which source controls encode tangent propagation and linearization?

**Known.** Sweep scopes carry a role-`0x43` group before a path group, or a profile carrier with a path and guide-face group without a separate selection group. Scalar owner ordinals zero through five independently identify the two path fractions, two rail fractions, twist and taper. Complete homogeneous output bindings establish the result family when the source operation is known.

**Need.** Controlled changes of the section representation, region membership, tangent propagation and linearization, with the remaining selections and scalar values held fixed.

**Note.** Remaining native definitions project as partial Sweeps. `feature.sweep-section-role-assumed` interprets a unique role-`0x43` group as the section. `feature.sweep-profile-carrier-assumed` retains the carrier identity without widening it to the whole Sketch. Required tangent and linearize flags assume false with `feature.sweep-path-tangent-assumed-false` and `feature.sweep-linearize-assumed-false`. Source records and unclassified controls remain in `sweep:source` with `feature.sweep-controls-uninterpreted`; input-body groups carry `feature.sweep-targets-native`. Missing source, operation, selection, scalar and result-mode joins have separate `feature.sweep-*-unresolved` codes or `feature.sweep-source-unavailable`. Result-family inference carries `feature.sweep-result-mode-inferred` and preserves a known Boolean operation.

### DR-621. Zero marker in the compact Sweep prefix

**Question.** What does the compact Sweep marker distinguish when it is zero?

**Known.** The ten-zero prefix and preceding word `1` occur with either marker value. The zero-marker form retains the six scalar-owner lanes and an existing-body group. The complete native parser requires marker `1`.

**Need.** A controlled change that toggles this marker while retaining the operation, selections and scalar controls.

**Note.** Partial projection reads the preceding word as Join with `feature.sweep-operation-prefix-assumed`. This reading does not depend on successful joins of unrelated scalars or selections. The marker and bounded source record fragments remain in `sweep:source`; complete native admission is unchanged.

### DR-485. Missing Decal image definitions

**Question.** Which source image and target does a Decal scope select when its image definition is absent or has an unparsed frame?

**Known.** The Decal scope identifies a presentation operation. Its prefix stores the image-asset reference, scope code, and target-group reference. The referenced image definition supplies the image basename.

**Need.** A save that preserves the selected image and target while its definition uses another framing form.

**Note.** The decoder retains the scope identity as an external image source and records `appearance.decal-definition-unresolved`. It preserves parsed prefix values in source properties and uses the target-group reference to select faces. Missing target bindings produce `appearance.decal-target-unresolved`. The fit-to-faces mapping of such a Decal carries `appearance.decal-mapping-assumed`.

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

### DR-489. UnStitch prologue flag and reference runs

**Question.** Which setting does the UnStitch selection flag encode, and which entities do the identity and source reference runs name?

**Known.** `f3d.md` §3.1 "An `UnStitch` scope separates selected faces" defines the three prologue forms, the three counted reference runs, and the face group. The identity run has one reference for each member face. The selection flag is one in scopes with a nonempty source run and zero in scopes with an empty source run.

**Need.** Saves that toggle chain selection and that select a whole body, with the resulting surface bodies identified.

**Note.** The decoder separates exactly the member faces of the face group and records `feature.unstitch-prologue-controls-assumed`.

### DR-488. UnStitch frames outside the three prologue forms and incomplete face groups

**Question.** Which controls do further UnStitch frames carry, and which faces does an incomplete face-group definition select?

**Known.** `f3d.md` §3.1 "An `UnStitch` scope separates selected faces" defines the ordered-reference table and its face group. The scope retains its ordered references in every frame.

**Need.** Controlled saves with further prologue forms and complete native definitions for the referenced face groups.

**Note.** The decoder keeps the neutral face separation. It records `feature.unstitch-frame-assumed` for a frame outside the three forms and `feature.unstitch-group-selection-assumed` for a missing or inconsistent face group, and retains the scope references as the native face selection.

### DR-480. Extrude termination through a compact Sketch-point identity

**Question.** How does an Extrude termination group use a compact Sketch-point selection to set its travel, including an offset and a two-sided extent?

**Known.** An Extrude scope can own a termination group whose member `N` has a 37-byte identity record `N+3`. The identity record stores ten zero bytes after its indexed header, then two u64 values at offsets 21 and 29. This is the compact Sketch-point layout in `f3d.md` §3.1. The first u64 is the record index of a Sketch, and the second is the persistent identity of a point record that this Sketch owns. The point's stored coordinates are `(-0.125, -0.13, 0)` cm. A placement record maps this point to `(0, -1.25, -1.3)` mm. The profile Sketch has an identity placement. The final floor at the termination uses one plane surface at `z = -1.3` mm. The Extrude input topology has ten faces. The final topology splits the floor into six faces on that surface. Entity-selection decoding admits this compact identity for Loft point groups; Extrude termination projection consumes face operands and WorkPlane selections.

**Need.** A second independent design with this termination form and known point selection, plus paired point-height, offset, and sidedness changes. Establish the travel plane and the relation that preserves the selected Sketch point when its adjacent faces change.

**Conflict.** The termination group role also carries face targets. A point at the floor height and several coplanar final faces permit different face assignments.

**Note.** The decoder keeps the termination group native and reports `feature.face-selection-native` and `feature.definition-incomplete`. DR-125 and DR-335 track counted face recipes.

### DR-481. Chamfer edge selection carried by bounded-face recipes

**Question.** Which edges do two bounded-face members of a Chamfer edge group select, and how do their boundary nodes assign the transition edges to each member?

**Known.** A Chamfer edge group with role `0x0000001000000000` can name two members. The compact identity prefix of each member names a local record. Each local record carries `bounded_face_recipe_data`, followed by a counted program with twenty parsed nodes. Both operands retain the same four preceding candidate faces. Their prefix references also name eleven further faces. References with two of the Design values have empty candidate lanes. The input topology has fifteen faces and thirty-eight edges. Each compact identity retains the same twenty-two transition edge candidates, in five runs of consecutive edge slots of five, three, six, three, and five slots, with one slot between adjacent runs. Each selected-face candidate has a four-edge preceding loop. The group has a 0.4 mm equal-distance Chamfer specification.

**Need.** Independent Chamfer designs with bounded-face members, including one selected face, two adjacent selected faces, and a face whose boundary splits before the operation. Establish the selected boundary, the treatment of tangent boundary edges, and the binding of empty prefix lanes.

**Conflict.** A complete twenty-node program, four face candidates, and a shared transition set permit several member assignments. Selecting every transition candidate assumes a group-level rule.

**Note.** The decoder retains the bounded-face operands and compact identity candidates. The group stays native with `feature.edge-selection-native`. DR-224 tracks Fillet transition ambiguities; DR-244 tracks treatment groups whose transitions supply no candidate set. The same form also occurs as a Chamfer group with two bounded-face members and 13 transition candidates. A Chamfer can also retain one unresolved bounded-face group beside a resolved edge group.

### DR-482. Source binding of timeline-owned Base Feature results

**Question.** Which records bind the source bodies and placement of a timeline-owned `Base Feature` when its stored body runs have a complete result binding?

**Known.** A recipe-absent scope can have a timeline owner. The owner has paste type `BA3059E4-4B23-4D02-9D18-E7BB95C991F3` or body-to-component type `56225BC7-69B7-446A-9AEA-6AA25BC7806E`. A paste-owned scope can have a 324-byte frame whose owner frame carries a null removal GUID. Such a scope contains parameter-body and result-body runs followed by a counted passive-reference run, shared metadata, and a result-record run. The paste GUID-pair reader stops at the counted passive-reference run and reports `feature.paste-sources-unresolved`. A paste-owned scope can also have a 485-byte frame. Each of these paste-owned and body-to-component scopes retains a resolved stored-body definition.

**Need.** A source-identity join for the passive-reference form, and a source and destination component join for the body-to-component form. Check the joins on two independent designs and compare the source and result topology, placement, and history transition. A consumed source requires its preceding-state identity.

**Conflict.** None.

**Note.** DR-260 tracks placement of a pasted copy. The result binding establishes the captured bodies. The decoder reports `feature.stored-bodies-recipe-absent` for these scopes.

### DR-483. Terminal marker in an ordinary Base Feature count field

**Question.** What does the terminal byte of the marked null field mean when it is `1` before the repeated-reference count in a Base Feature envelope?

**Known.** A Base Feature scope record can have class pair `329/264`, frame length `318`, and a history transition. Its first two marked fifteen-byte entries name two records. Both targets have Body presentation-owner type `D3937028-C20C-4E65-B010-94AD418A5C20`. The eleven-byte field after those entries is a marked null reference with flag byte `0` and terminal byte `1`. A u32 count `1` follows, then a marked reference to the target of the first entry, a shared metadata record, and a result record. The decoder reads the ordinary result-body envelope when the terminal byte is `0`, and leaves this frame's payload uninterpreted. The history transition binds one body and supplies the resolved Base Feature result set.

**Need.** A rule for the terminal marker and the roles of the two initial reference runs. Compare a controlled capture, paste, and body-to-component operation with equal geometry, including their referenced Body records and preceding ASM state. Confirm the role rule in a second independent design before selecting a source or construction operation.

**Conflict.** None.

**Note.** The specialized scope payload stays uninterpreted. The result bodies remain available with `feature.stored-bodies-recipe-absent`.

### DR-492. Mesh face-group method, tolerance and selection semantics

**Question.** How do the method code, dimensioned boundary tolerance, two mesh-GUID references, leading Combine UUID and ordered UUID run define the historical face-group edit?

**Known.** `f3d.md` §3.1 "The mesh face-group parameter scopes" defines the registrations and prefix sequence. The method carrier stores `0` and `1`. Boundary tolerance has a length unit. The second mesh-GUID reference precedes the counted UUID run. Combine's leading UUID can also occur in that run. The construction member references a mesh body whose owning scope can precede the edit.

**Need.** Independent saves that vary method, mesh scale, selected-group order and cardinality, including edits to an existing subset and saves with absent inactive settings. Bind each UUID to a historical triangle group through its mesh-GUID state.

**Conflict.** The user-facing tolerance is described as a proportion of mesh extent; the native parameter is dimensioned as length.

**Note.** The decoder reads method `0` as Fast and `1` as Accurate with `feature.mesh-face-groups-method-assumed`. It preserves dimensioned tolerance as an absolute millimetre distance with `feature.mesh-face-groups-boundary-distance-assumed`. It retains the scope as the mesh source expression with `feature.mesh-face-groups-source-assumed` and a complete UUID run as ordered group selectors with `feature.mesh-face-groups-group-roles-assumed`. The first mesh reference and leading Combine UUID survive in source properties with `feature.mesh-face-groups-related-role-unassigned` and `feature.mesh-face-groups-group-hint-role-unassigned`. A missing, ambiguous or unsupported first mesh registration reports `feature.mesh-face-groups-related-target-unverified`. Unassigned settings and incomplete member runs retain their native evidence with distinct loss codes.

### DR-493. Partial PathPattern seed and placement semantics

**Question.** Which native evidence distinguishes a feature index from a coincident topology identity in a role-`0x4` PathPattern seed, and how does the reordered settings instance table define placement?

**Known.** Role-`0x4` seed members carry a 29-byte compact identity. The decoder requires a selected feature to precede the pattern in the authored timeline, assumes the feature-index reading, and reports `feature.path-pattern-feature-seed-assumed`. Reordered references place count and distance owners before the seed group. The decoder reads fixed controls of a 298-byte settings record by the 267-byte form and reports `feature.path-pattern-reordered-settings-assumed`. Independently decoded count, source distance, adjacent spacing, path, flip, orientation, and start parameter survive in a partial curve-driven definition with `feature.path-pattern-definition-partial`. An unresolved seed retains its native identity with `feature.path-pattern-seed-unresolved`. A marked path reference survives an unclosed group join with `feature.path-pattern-path-unresolved`. A nonpositive source distance and a count outside the neutral integer range stay in their source parameters with `feature.path-pattern-distance-nonpositive` and `feature.path-pattern-count-out-of-range`. An unavailable start parameter reports `feature.path-pattern-start-unresolved`.

**Need.** Independent feature and face seeds with colliding identity values, and controlled edits that vary instance count and distribution in the reordered settings form.

**Conflict.** A compact identity can also match a topology identity in the captured history.

**Note.** DR-290 tracks the scalar controls at settings offsets 94 and 124. Unavailable placement controls report `feature.path-pattern-settings-unresolved`.

### DR-550. Captured component-position semantics and frame

**Question.** Do Position version-six row matrices represent captured occurrence poses in each occurrence's assembly context? Does marker 1 select identity? How do nested contexts and occurrence identity lists affect the frame?

**Known.** `f3d.md` §3.1 "A Position scope has type GUID" defines the counted rows, matrix omission marker, suffix, and typed locator backlinks. [Capture Position](https://help.autodesk.com/view/fusion360/ENU/?contextId=ASM-CAPTURE-POSITION) captures the positions of moved or rotated components. [Snapshots](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/fusion_Snapshots.htm) creates snapshots when transforms are pending. [Occurrence.transform2](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/fusion_Occurrence_transform2.htm) gives orientation and position in the occurrence's assembly context. The decoder assumes these semantics and centimetre translations for the binary matrices and reports `feature.position-semantics-assumed`. It reports `feature.position-identity-assumed` for omitted matrices. It retains unresolved selectors and transforms and reports `feature.position-occurrence-unresolved`. Final assembly placement remains the stored placement.

**Need.** Controlled saves with one translated occurrence, one rotated occurrence, two independent poses, nested occurrences, a repeated selector, and a return to identity. Compare the row matrices with API snapshots and `transform2` values at each timeline state. Vary an occurrence identity list independently.

**Conflict.** None.

**Note.** Native rows retain the source coefficients and selector references. Neutral rows retain source order, repeated selectors, and unresolved occurrences.

### DR-490. Operand containers of unqualified Assemble connectors

**Question.** Which occurrence or document contains a connector when its scope retains the connector reference and frame but its operand qualifier does not decode?

**Known.** The Assemble connector grammar in §3.1 of `f3d.md` retains two native references, their operand-local frames, alignment scalars, and facing. Each reference identifies a source record within the Design stream.

**Need.** Controlled assemblies that identify both selected components and vary the operand paths independently of the connector frames and alignment values.

**Note.** The neutral joint uses the native scope identity as its operand object and the stored construction reference as its native element path, with a missing document container. `assembly.connector-operands-assumed` names this operand-context assumption. `assembly.motion-binding-assumed` records the adjacent-motion or fixed-relation reading from DR-250. The timeline feature names the retained joint before archive operand descent. A missing document identity supplies no implied occurrence placement.

### DR-491. Source membership and container of an unbound Component Insert

**Question.** Which source occurrence and containing component does a Component Insert scope select when its relation carrier or authored placement path remains unresolved?

**Known.** The shared identity and matrix scope forms in §3.1 of `f3d.md` retain an occurrence identity, one relation reference, and a scope-local placement. This includes the 256-, 257-, 261- and 267-byte identity scopes and the 385-, 389- and 399-byte matrix scopes with legacy tails. The carrier contributes the source occurrence role used by external-reference binding. The scope occurrence identity can repeat across scopes.

**Need.** Controlled assemblies with independently varied source occurrences, containing components, and placement matrices, including a retained relation with an unresolved source document.

**Note.** Each authored insertion scope with a decoded placement can produce one unresolved-prototype occurrence at the document root. The per-scope membership reading is assumed. `assembly.component-insert-container-assumed` records that container-frame reading. A feature with unresolved source membership uses its native selection in a neutral component insertion and reports `feature.component-insert-membership-assumed`. The native scope retains the relation reference, occurrence identity, and decoded placement.

### DR-506. Motion roles of additional Assemble owners

**Question.** How do additional rotation, translation, yaw, and roll owners select enabled motion axes, limits, and rest positions when the joint motion is unresolved or has multiple freedom lanes?

**Known.** The counted owner run in §3.1 of `f3d.md` names additional owners in local order. Parameter source kinds identify rotation and translation minimum, maximum, and rest values, and yaw and roll values. A two-reference run can contain a minimum and a rest value. Angular evaluated values use radians and linear evaluated values use centimetres. A scope can retain these owners when its joint motion remains unresolved.

**Need.** Controlled saves that vary one enabled axis, one limit, or one rest position at a time and retain all joint and motion lanes. Distinguish the authored owner values from enabled solver limits for combined freedoms.

**Note.** The neutral relation uses the decoded adjacent joint motion with a fixed relation fallback. `assembly.additional-owner-motion-assumed` names the assumption that the added owner-to-motion interpretation remains native. The feature retains each source parameter expression under `parameter:NAME` in its source properties. The native scope retains the counted references, owners, and evaluated values.

### DR-507. Extended version-three Assemble tail reference

**Question.** What constraint or solver role does the additional version-three tail reference select?

**Known.** The extra nonnull reference follows the first tail reference and selects type `074F9459-89C2-4B7D-8AB4-DCE67AE944FE`, version 1. The tail consumes its complete bounded scope. Its two boolean state bytes and the adjacent joint retain their independent fields. The native alignment retains the extra target and marker offset.

**Need.** Controlled saves that vary the referenced object while preserving the joint, connectors, owner values, and motion record. Establish whether the target changes the joint's enabled constraints or solver membership.

**Note.** The neutral relation uses the decoded joint motion with a fixed fallback. `assembly.extended-tail-assumed` names the assumption that the additional tail reference adds no further neutral constraint. Source parameter expressions remain under `parameter:NAME` in feature source properties. A JointOrigin coordinate system supplied by an extended-tail connector retains its source references and parameter expressions. The connector and origin share their stream, target index, transform, and transform location.

### DR-508. Typed spelling of the extended Assemble tail

**Question.** Does the extra typed tail reference impose a different relation role from its plain spelling?

**Known.** A standard typed scope tail has three 51-byte references with inline type GUIDs. The extended tail adds a fourth reference after the first. A uniform 51-byte width shifts the previous-history word and each later field by 51 bytes. The additional target's inline GUID agrees with its registered type.

**Need.** Controlled saves that preserve the same extended-tail target and joint while changing the reference spelling. Establish the additional reference's type-directed relation role.

**Note.** `assembly.extended-typed-tail-assumed` names the assumption that the added reference uses the common typed spelling and retains the plain spelling's role. `assembly.extended-tail-assumed` separately names the neutral constraint assumption. Both source forms retain their target, marker offset, and source expressions.

### DR-553. Treatment recipe references with empty preceding face lanes

**Question.** Which historical face does a treatment recipe reference select when its persistent names bind to an empty face lane in the feature-input state?

**Known.** The first-side reference ordinals select preceding face-boundary lanes. A Fillet recipe operand can have a first lane that names one face of the input state and an empty second lane. The decoder retains the recipe programs, persistent names, and transition candidates.

**Need.** The reference-to-face binding across the preceding topology revisions, with the selected faces and edges identified in independent designs.

**Conflict.** None.

**Note.** The unresolved members retain their native identities and report `history.edge-midpoints-unresolved`. DR-164 tracks selectors over populated lanes; DR-224 tracks incomplete transition proofs.

### DR-570. Additional SplitFace method controls

**Question.** Which splitting methods do SplitFace method-control tuples outside the documented surface-intersection, closest-point, and along-vector forms select?

**Known.** The method control stores a u32 family, a u8 projection, and a u8 extension after the nullable direction reference. The recognized tuples are recorded in `f3d.md` §3.1. Other tuples remain available as raw controls.

**Need.** Controlled saves that change one splitting method setting at a time and identify the resulting tuple.

**Conflict.** None.

**Note.** The decoder keeps the SplitFace operation with an `Unresolved` method and records `feature.split-face-method-unresolved`.

### DR-571. Along-vector source binding

**Question.** How do an along-vector carrier's ordered input references identify a neutral direction source?

**Known.** The carrier stores a finite three-component vector and ordered input references. The vector can be retained when the source feature or its definition is unavailable.

**Need.** Independent identity joins from carrier inputs to neutral axis, plane, edge, or face selections across supported document states.

**Conflict.** None.

**Note.** The decoder retains the vector, complete reference qualifiers, and the readable prefix of an incomplete input run in a typed along-vector method. The native payload distinguishes a complete run from an unavailable or truncated run. External targets do not select same-numbered local records. `feature.split-face-direction-unresolved` records the unavailable neutral source; an incomplete local reference partition also records DR-572.

### DR-572. Incomplete SplitFace reference partitions

**Question.** How do additional ordered references outside the tool group, target group, and decoded along-vector references affect a SplitFace operation?

**Known.** The tool group precedes the target group and their members appear contiguously. An along-vector scope can carry a direction carrier and its input selections after the group members.

**Need.** Controlled scopes with additional reference members whose records and roles identify their relation to the SplitFace operation.

**Conflict.** None.

**Note.** The decoder projects the readable method and selections and records `feature.split-face-groups-unresolved` while the full ordered partition is unproved.

### DR-573. Unresolved SplitFace tool selections

**Question.** Which role-`0x0000002100000000` selections identify splitting geometry beyond resolved paths, WorkPlanes, and historical face selections?

**Known.** The role can carry path, plane, or face selections. Its ordered native selection remains available when a unique neutral geometry cannot be established.

**Need.** Controlled selections that vary the native entity type and record path independently.

**Conflict.** None.

**Note.** The decoder projects the SplitFace operation with its readable method and records `feature.split-face-selection-unresolved` for unresolved tools or targets.


### DR-574. SplitFace face projection failures

**Question.** Which face selections require additional projection rules?

**Known.** A face-selection projection can fail while decoding a SplitFace group.

**Need.** A face selection and its complete registered operand frame for each unsupported projection.

**Conflict.** None.

**Note.** The decoder retains the native group and records `feature.split-face-selection-projection-failed` with the projection error.

### DR-554. Interior null timeline items

**Question.** Does a null item before a non-null reference occupy a timeline position?

**Known.** A counted feature-timeline run can contain a one-byte null before another local reference. The local references retain their serialized order.

**Need.** Controlled saves with a removed or suppressed timeline item and a visible timeline, including a save with a null between two readable references.

**Conflict.** None.

**Note.** The decoder assumes the null names no item and reports `feature.timeline-null-slot-assumed`.

### DR-530. Placement of object-prelude occurrences that no component path names

**Question.** What gives the local placement of an object-prelude occurrence, whose carrier GUID follows the ordinal directly, when no component-path record names it as its target?

**Known.** An occurrence carrier can have an object property prelude, a one-byte zero ordinal prefix, and one closing placement. Its carrier GUID follows the ordinal directly. The closing component reference precedes the placement flag. A component-path record that names such an occurrence as its target stores a world pose that differs from the closing matrix. In a stream with a component-path record, where every record is read as a local path form and names one unique expanded occurrence, the closing matrices of the object-prelude occurrences that no record names place their solved bodies coaxially with the bodies they mate with. A single such carrier in a stream with no component-path record rotates the X axis of a solved body to Z; the mating body has axis X. A carrier with a two-byte prefix and a source marker is placed from its closing matrix under the assumption that the matrix is in the component frame. The leading-source and closing-local placements in the F30 generation are separate fields in another complete envelope.

**Need.** A controlled object-prelude save with a moved component, with and without a component-path record for it, its original and solved BREP frames, and an independent current placement. Determine the reference frame of the closing matrix and the body-frame correction before admitting that matrix as a body placement in a stream with no component-path record.

**Conflict.** Treating the closing matrix of an object-prelude carrier whose GUID follows the ordinal directly as a solved-body local placement, in a stream with no component-path record, gives the incorrect mating placement above.

**Note.** The decoder retains the native carrier. It assumes the closing matrix is the local placement in the component frame of the owner, and records `assembly.local-occurrence-object-prelude-assumed`, only when the carrier's Design stream stores at least one component-path record, every component-path record of that stream is read as a local path form, every one names one unique expanded occurrence (a pose chosen among several instances does not), and none names the carrier as its target. Otherwise the matrix-bearing occurrence keeps its stored body frames and the existing `assembly.local-occurrence-unplaced` loss. The condition applies to each Design stream separately. Identity frames retain their definition frame. Ancestor placement must also resolve. An object-prelude occurrence whose two-byte prefix carries a source marker (flag 0 or 1) is placed from its closing matrix without that proof, with the assumption loss `assembly.local-occurrence-marked-prelude-assumed`. A carrier that an assembly alignment operand path names, with no frame sense, is placed from its closing matrix and carries the same assumption loss; an occurrence that an assembly operation places from an operation matrix does not.

### SM-03. ContourFlange travel, thickness side and rule entries

**Question.** Which controls select ContourFlange sidedness, thickness orientation, body or component result, and the rule's bend-radius entry?

**Known.** The registered ContourFlange prefix carries a vector before the AlongDistance owner, an operation word, an unassigned control before the source carrier and an inline thickness copy. Signed distances and the linked rule's first expression/value, material scalar and explicit material-trailer thickness decode independently. The projection reads the vector as travel direction, a single distance as one-sided travel, the documented SideOne default as the left side of the ordered profile, operation word four as an independent body, the first rule value as inside bend radius and the material scalar as neutral-axis factor. Each reading carries its named `feature.contour-*` assumption loss. The explicit BendRadius parameter takes precedence over the rule entry. A missing rule uses the inline thickness copy with a separate assumption loss. A bounded unique rule reference before the exact settings owner tail is loss-coded when the versioned optional-slot grammar does not match. Missing definitions use a matching scoped owner's cached database-centimetre scalar with a named loss. When both owner and definition frames are absent, a unique registered parameter-owner reference following the vector supplies the AlongDistance role through a separate assumption loss. Missing carrier or settings links preserve independently decoded travel. Source-region selections retain the complete native profile program.

**Need.** Controlled saves must vary extent, thickness side, result choice, radius expression and material factor independently, with the stored controls and resulting topology compared.

**Note.** The API's SideOne default and per-side symmetric distance are described in [ContourFlange orientation](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/fusion_ContourFlangeInput_orientation.htm) and [Sheet Metal Features](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/SheetMetalFeatures_UM.htm). [Sheet Metal Rules](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/SheetMetalRules_UM.htm) distinguishes thickness, bend radius and neutral-axis factor.

### DR-562. Occurrence of a cross-component Combine tool body

**Question.** Which occurrence of its component supplies the body that a cross-component `Combine` tool names?

**Known.** The browser node of a tool names the body entity suffix and the creating scope. The history-only body-map pair of that suffix names the BREP blob and body selector. The operand-list body-selection record stores an occurrence list before the body-node list.

**Need.** A design that places one component more than once and combines the body of one placement, with the occurrence-list entries of the tool decoded against the component instance records.

**Conflict.** None.

**Note.** A generated tool identifies its generating feature and body-map pair only.

### DR-564. Reference-framed EdgeFlange width and boundary programs

**Question.** How do wrapper members and boundary programs select the flange width law across reference-framed operation variants?

**Known.** The operation prologue and closing group run have equal edge counts. Their wrapper and main-group slots pair by ordinal. A five-member wrapper carries a main-group reference, two boundary-group references and two owners named `EdgeOffset_1` and `EdgeOffset_2`. The signed owners retain zero and negative lengths. The boundary references retain independent source programs.

**Need.** Controlled saves that vary each boundary object and each signed offset independently, and compare full-edge and offset modes while preserving the main-edge order.

**Note.** `feature.edge-flange-signed-offsets-assumed` records the wrapper-linked offset association. `feature.edge-flange-full-width-assumed` records full-edge width for reference-framed wrappers without a decoded offset pair. `feature.edge-flange-boundary-reference-native` retains decoded boundary programs. `feature.edge-flange-boundary-scope-reference-assumed` retains the owning scope when a boundary definition is unresolved. A missing construction stays neutral with independent fields and `feature.edge-flange-reference-envelope-unresolved`.

### DR-565. Independent EdgeFlange parameter recovery

**Question.** Which saved parameter value is authoritative when an EdgeFlange owner or definition is unresolved?

**Known.** The operation names height and angle owners. A parameter definition can retain its owner backlink, and a parameter owner stores an independent finite cached scalar. Length values use database centimetres and angular values use radians. The display-unit token is stored separately.

**Need.** Controlled saves with a changed definition, changed owner cache or unresolved display-unit token while the other parameter fields remain fixed.

**Note.** `feature.edge-flange-parameter-owner-backlink-assumed` permits a unique definition backlink when its owner frame is absent. `feature.edge-flange-cached-parameter-assumed` permits a cached scalar when its definition is absent. `feature.edge-flange-parameter-unit-assumed` permits the database unit convention when the display-unit token is unresolved. A missing value reports `feature.edge-flange-parameter-unresolved` and preserves other inputs.

### DR-566. Unresolved EdgeFlange height targets

**Question.** Which neutral construction does a retained to-object height selection identify when its source target is unresolved?

**Known.** A to-object height carries a target group, target selection and signed offset owner. Each input has an independent source identity. A local WorkPlane or WorkPoint target has its own construction scope.

**Need.** Controlled to-object flanges whose selected construction remains external or unresolved while the signed offset is retained.

**Note.** `feature.edge-flange-height-target-native` retains the decoded target selection program. `feature.edge-flange-height-target-scope-assumed` retains the owning scope program when that target selection is unresolved. The target and signed offset remain independent neutral construction inputs.

### DR-655. Profile loop entries without a sketch-curve record

**Question.** Which record carries the curve of a profile loop entry when no sketch-curve record of the owning Sketch has the entry's primary identity?

**Known.** A typed-reference profile selection can store one loop of seven entries under a Sketch owner. The Sketch holds a line record for the primary identity of every entry except the fifth, and one further line record whose identity no entry names. Without the fifth entry, the six remaining lines form one closed boundary once the lines of the fourth and seventh entries end at the line of the first entry. No uint64 property in the stream holds the primary identity of the fifth entry outside the selection and ASM history records. A loop can have one entry under a Sketch that holds twenty circle records whose primary identities increase in steps of two. The entry names the next value in that step, and no circle record carries it. A selection can list fifteen entries with consecutive even primary identities under an owner that no Sketch placement, curve record, or point record names. The seventh, ninth, tenth, fourteenth, and fifteenth of these identities are carried by curve records of another Sketch, which has its own placement and selection. A typed-reference profile selection can have a sketchless chain record (`f3d.md` §3.1). Its asset GUID is not the asset GUID of the containing Design, and no archive member of its project has it as a document or asset GUID. Its decimal owner text names no entity suffix of the Design stream, so the owning Sketch belongs to another design, and its seven primary identities match no sketch-curve record of the Design. Four other records of the Design store the same asset GUID followed by one further GUID, the same in each record. The Extrude of this selection is the first timeline feature, and no Sketch precedes it. A compact sketchless profile selection can have an asset GUID other than that of the containing Design and a GUID as its owner text.

**Need.** A stored carrier or a derivation rule for absent curves whose surviving members fail the complete directed-boundary and transition certificate in `f3d.md` §3.1. The Sketch that a typed-reference owner text names when no Sketch placement, sketch-curve record, or sketch-point record of the Design stream carries that owner. The document that owns the asset GUID of a sketchless chain record's selection when no archive member carries that GUID as a document or asset GUID.

**Note.** A selection whose surviving members pass the stale-identity certificate in `f3d.md` §3.1 binds its certified regions with `feature.profile-region-identities-stale`. Each other selection stays native with `feature.profile-selection-native`. A compact sketchless profile selection also reports `feature.extrude-profile-sketch-absent`.

### DR-670. Carrier state of a Hole on a reconstructed plane

**Question.** What do the carrier position and the carrier point parameters of a face-point Hole store when its point record names a reconstructed plane at another position?

**Known.** The carrier has `refType` `19`. The point record has `refType` `17` and its position lies on the reconstructed plane. The carrier position differs from the point-record position, and the carrier point parameters share their first value with the point-record parameters. The drilled cylinder lies on the point-record position.

**Need.** A second Hole whose carrier and point record disagree, with the Hole saved before and after the plane is moved.

**Note.** The entry point is the point-record position. The carrier position is retained in the native carrier record.

### DR-671. Second direct selection of a Hole on a reconstructed plane

**Question.** What does the second direct support-face selection of the same Hole select, and how does it relate to the carrier input?

**Known.** The Hole scope references two direct support-face selections after its point-data records. The first is the sole input of the point record. The carrier input names the second. The second selection owns a record with a sketch name and a sketch frame in place of a reconstructed plane record.

**Need.** A second Hole whose carrier input names a sketch selection, and a save that moves the sketch.

**Note.** The sketch frame is not used for placement.

### DR-760. Structured Chamfer recipes on faces with several deleted boundary edges

**Question.** Which input edge does a member of a counted Chamfer group select when its standard edge recipe has a two-sided structure, complete counted-prefix operand ordinals, and primary-reference face boundaries with several transition-deleted edges?

**Known.** The scope binds to one ASM history through the BREP of its edge-recipe faces, as DR-150 records. Each counted operand keeps one program ordinal across its names and their Design-reference lists. The member recipe resolves against the input state of that history. Each side lists boundary-edge counts and topology triplets of incident edges and vertices. Two groups of one scope can have twenty members each and the same input state. Their transition deletes 34 edges. Each group resolves the same six edge slots. Fourteen members retain multiple candidate edges or unresolved reference contexts. Their primary-reference face boundaries hold eight or more transition-deleted edges. The projector retains the six resolved edges and fourteen operand identities in a partial historical selection.

**Need.** A rule that maps the side entries and topology triplets, with complete prefix-operand grouping, to one input edge, or a group assignment that distinguishes selected edges from the other transition-deleted edges.

**Conflict.** None.

**Note.** DR-342 tracks chamfer recipes whose transition selections conflict.

### DR-705. Cut Extrude whose removed planes disagree with the sampled regions

**Question.** Which rule selects the atomic regions of a Cut `Extrude` whose counted selection matches no arrangement face when the planes that the Cut changed select more regions than its transition certifies?

**Known.** A Cut `Extrude` can have a Sketch of three profiles and an arrangement of 14 atomic regions. The Cut changes three planes parallel to the Sketch. The regions whose interior points a changed plane held before the Cut and not after it do not pass the exact transition certificate. Its counted selection lists four rings of four, four, one, and four curves, and one ring has incidence words greater than two. No ring matches an arrangement face.

**Need.** The rule that relates the incidence words greater than two to the removed regions, or a certificate that selects the removed regions without the sampled points.

**Note.** The selection stays native with `feature.profile-selection-native`.

### DR-706. Cut Extrude with atomic regions that have no sample point

**Question.** How are the atomic regions of a Cut `Extrude` found when the arrangement has a region with no point more than one model tolerance from its boundary?

**Known.** A Cut `Extrude` can have a Sketch of 54 profiles and a counted selection of eight rings of three curves. The arrangement has 72 faces. The sampler rule of the cap certificate (`f3d.md`) reads a narrow region through a grid point or a row midpoint. A region narrower than two tolerances has no sample, and no removed-region candidate forms for it. The rings come in pairs of the same three curves in opposite order.

**Need.** A certificate that does not sample regions narrower than two tolerances.

**Note.** The selection stays native with `feature.profile-selection-native`.

### DR-791. Placement carriers of 393- and 399-byte Component Insert scopes

**Question.** Which occurrence placement grammar does the 527-byte carrier of a 393- or 399-byte Component Insert scope use, and which occurrence does that scope select?

**Known.** The 393-byte frame stores the fields of the 404-byte frame through its null GUID. A 399-byte frame with fourteen zero bytes after its indexed header stores the fields of the 400-byte frame through its null GUID. Both frames name a 57-byte relation whose first reference names a 527-byte placement record. That record stores, at its offset 307, a rigid matrix equal to the scope transform, and its last LP-UTF16 GUID equals the role of one external-reference entry. One such record and the legacy typed identity placement of a 261-byte scope can have the same role. The legacy typed and repeated-target placement grammars of §1.4 do not admit the 527-byte record, and the external-reference table does not place its entry from it.

**Need.** The grammar of the 527-byte record before its repeated target, and an assembly that inserts one external design twice, so that the pose of the second occurrence can be checked against final geometry.

**Note.** These scopes keep native Component Insert features with `feature.definition-incomplete`.

### DR-575. SplitFace direction carrier type disagreement

**Question.** What does an inline target type that disagrees with the registered type of a SplitFace direction carrier mean?

**Known.** The inline GUID names the target record type. The primary index and type registration identify the point-direction carrier and its bounded member frame.

**Need.** Controlled files that independently identify the carrier when its inline qualifier and registration differ.

**Conflict.** The reference qualifier and the registered carrier type make different type claims.

**Note.** The decoder retains the uniquely registered point-direction carrier and its readable vector as the most likely interpretation, preserves the original qualifier, and records `feature.split-face-carrier-type-assumed`.

### DR-780. Spatial Sketch frames without a nested region selection

**Question.** Which profiles does an Extrude sketch-profile frame on a spatial Sketch select when the frame stores no nested region selection?

**Known.** An Extrude frame on a spatial Sketch can store a nested region selection. Their selected regions include hole loops, loops of projected curves with nonzero secondary identities that bound no spatial profile and loops of two coincident full circles.

**Need.** A frame on a spatial Sketch without a nested selection, and the stored body of its Extrude transition.

**Note.** On a Sketch with more than one profile, the decoder states every distinct profile of such a frame only when the far caps of the Extrude transition certify their union; otherwise the frame stays a native spatial-Sketch selection with `feature.profile-selection-native`. On a Sketch with one profile, and for profiles that the selection groups or a nested region selection name, the decoder states the profiles when the far caps do not certify them, with `feature.spatial-profile-transition-assumed`.

### DR-781. Planar Sketch selections with secondary curve identities

**Question.** Does a nested profile-region selection of a planar Sketch frame whose members store nonzero secondary identities name planar profile loops the same way as one whose members store zero?

**Known.** Planar Sketch frames can store such selections. In a spatial Sketch frame, a member's primary and secondary values match a curve record's persistent identity pair.

**Need.** Planar frames with nonzero secondary members whose selected regions are proven against the stored Extrude bodies.

**Note.** The decoder resolves a planar frame whose nested selection has a member with a nonzero secondary identity as a frame without a nested selection.

### DR-810. ReplaceFace control and partial support selections

**Question.** Does the prologue Boolean expand the target selection to tangent faces, and which selection applies when a referenced support definition is incomplete or names a construction plane, what does the flagged-prefix word mean, and what does the trailing identifier select?

**Known.** The flag-01 prologue retains a u32 word before its Boolean. Each form retains a Boolean, separate ordered replacement and target group runs, and a trailing identifier. Replacement groups use roles `0x09` or `0x21`; target groups use role `0x10`.

**Need.** Controlled saves that change tangent chaining, construction-plane support, the flagged-prefix word and the trailing identifier independently, with complete definitions for each selected group.

**Note.** `feature.replace-face-flagged-prefix-word-assumed` retains the word as auxiliary metadata while applying the selections. `feature.replace-face-tangent-chain-assumed` retains the Boolean as target tangent expansion. `feature.replace-face-frame-assumed` uses owned group roles when the prefix is unresolved. `feature.replace-face-selection-assumed` keeps scope selections for incomplete or ambiguous definitions. `feature.replace-face-groups-aggregated-assumed` keeps several group references as one scope selection. `feature.replace-face-overlap-unresolved` retains native selectors when the resolved memberships overlap. `feature.replace-face-support-assumed` retains entity support through its native group. `feature.replace-face-identity-assumed` retains a nonnull identifier as auxiliary metadata.

### DR-665. Planar face of an Extrude profile selection with an unbound member identity

**Question.** Which planar face does an Extrude profile selection member select when its local identity is bound to no ASM entity of the history?

**Known.** The member's local identity is read as an entity slot of the preceding state. This read can name a loop or coedge of a cylinder wall. A profile face is planar, so the cylinder wall is not the selected face.

**Need.** A save in which a bound and an unbound member select the same planar face, or a save that stores the planar face's own identity beside the unbound member.

**Note.** A selection whose one face has a cylinder, axis, or radius carrier row and no plane row stays native and reports `feature.extrude-profile-face-curved` with the entity read and the face.

### DR-790. Design role of saved free edges

**Question.** Which Design meaning does a saved free edge of an ASM BREP blob have: visible model geometry of the component, or an edge that the kernel keeps for a timeline feature?

**Known.** A saved free edge is an `edge` record in the saved top-level range of the ASM stream with a null owner coedge (`asm.md` §5.2 "**Edge (98 B):**"). No coedge, loop, or face uses it. A design can store from 4 to 912 saved free edges. The decoder emits each one as one wire body, and the count per design equals the count of null-owner-coedge `edge` records in the selected BREP blob. No Design body binding, body member, or body visibility record names a saved free edge. The `affected_saved_edges` property of Extrude, Fillet, Chamfer, and Thread transitions lists them. A saved free edge can have the same end positions as an edge of a body with faces in the same design, and it can be a closed edge.

**Need.** A Design record that names a saved free edge, or a rule that relates a saved free edge to the feature that creates it, and the visibility of that edge in the component.

**Note.** `f3d.md` §3.1 "A local occurrence is a child of each occurrence" excludes saved free-edge bodies from component body coverage. A saved free-edge body without a product-definition owner is reported as `assembly.saved-free-edge-body-unowned`.

### DR-812. Draft parting controls

**Question.** How do anchor kinds `2` and `3`, their mode word, the auxiliary scalars, and the boundary flag define fixed and moving parting-line Drafts?

**Known.** These prefixes name the angle owners, drafted-face group, pull group, tool group, and an optional fixed-edge group. The public Draft tool reference defines fixed and moving parting lines, fixed-line side modes, moving-line angle directions, and fixed edges. Both stored angle lanes can be equal while the mode word differs. The file-backed legacy layout has anchor kind `2`, mode `1`, a zero second angle, and an absent fixed-edge reference. The reading for other parting controls follows the group roles and target types. Synthetic cases exercise mode `2`, including kind `3` with a nonzero second angle and a present fixed-edge reference.

**Need.** Controlled changes of each parting-line option with independently identified signed angles, tool curves, and fixed edges.

**Note.** The decoder reads these anchor kinds as parting-line Drafts and reports `feature.draft-parting-controls-partial`. It transfers the first angle, drafted-face selection, native tool selection, and resolved pull. The native construction retains both angles, all control values, and the fixed-edge reference.

### DR-811. Partial Draft anchor construction

**Question.** Which anchor selection applies when a Draft's pull construction, scalar lanes, or referenced groups are incomplete or ambiguous?

**Known.** The owning scope and complete same-stream groups retain native selector identities. A unique referenced owner at local ordinal zero retains the signed first angle in radians.

**Need.** Complete anchor definitions and unique selector bindings for these forms.

**Note.** The decoder retains a partial neutral-plane Draft with native anchor and face selectors and any recovered angle, and reports `feature.draft-neutral-anchor-assumed`. A finite angle outside the neutral slope domain remains native and reports `feature.draft-slope-out-of-range`.

### DR-792. Pose of an unresolved legacy insertion

**Question.** Which current occurrence pose accompanies a legacy insertion whose source carrier or operation table remains unresolved?

**Known.** The insertion scope stores an identity or rigid insertion frame. A separate occurrence carrier or operation table can store the current pose. The scope retains its occurrence identity and relation reference independently of those records.

**Need.** Controlled assemblies that move an inserted occurrence while retaining the insertion scope and independently varying its source carrier and operation table.

**Note.** `assembly.legacy-insert-frame-assumed` names the assumption that an unresolved occurrence uses its scope-local insertion frame as its current frame. `feature.component-insert-membership-assumed` separately names unresolved source membership. A decoded source binding takes precedence over this fallback. The public API separates [initial position](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/fusion_Occurrence_initialTransform.htm) from [assembly-context position and orientation](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/fusion_Occurrence_transform2.htm).

### DR-800. Short compact Coil controls

**Question.** Which options do the two control bytes of the 454-byte ten-reference Coil scope encode?

**Known.** The scope stores `1,1` in the control pair and uses the compact operation, extent, left-hand, section-position, and section-shape lanes. The parameter owners identify the driving dimensions. The reference table names a body-target group, its member, a persistent support selection, and a solved placement construction.

**Need.** Fusion-authored paired saves that vary each control independently while retaining the extent, section, placement, and Boolean result.

**Conflict.** None.

**Note.** The decoder applies the compact discriminator meanings and reports `feature.coil-short-controls-assumed`. It retains signed driving dimensions and the checked solved placement. An unbound placement remains native. A missing or ambiguous target group retains the Coil with an unresolved Boolean selection and reports `feature.coil-target-unresolved`.

### DR-813. Cylinder support-local frame and reference roles

**Question.** Which extended Cylinder configurations use a different base frame, and what does the counted reference select?

**Known.** The extended scope stores a counted reference and a separate support placement target. The expanded form also stores a rigid matrix. The support frame composed on the left of this matrix agrees with the saved cylinder axis and transverse centre in resolved cases.

**Need.** Controlled saves that vary the support frame, base centre, and selected construction independently.

**Note.** The decoder uses the support frame directly for the compact form and composes it on the left of the local matrix for the expanded and typed forms. Both readings report `feature.cylinder-local-frame-assumed`. The native construction retains the counted reference and any local matrix.

### DR-596. BoundaryFill cell geometry

**Question.** Which boundaries define each selected-cell record?

**Known.** The scope names the selected cells in an ordered reference list. The selected-cell type differs from the body-region group type.

**Need.** A relationship between each selected-cell record and the set of body and plane boundaries that enclose it.

**Note.** The decoder retains the cell identities and reports `feature.boundary-fill-cells-native`. An unavailable cell list and body-region partition report `feature.boundary-fill-cells-unresolved`.

### DR-597. BoundaryFill tool bindings

**Question.** How do the group and plane lists select neutral tools?

**Known.** A role-4 group at the start of the scope reference table contains ordered body-recipe members. The prefix stores separate group and plane lists.

**Need.** A same-state identity join for the plane selectors and an assignment for each body-group reference outside the role-4 group.

**Note.** Unresolved neutral tool selections report `feature.boundary-fill-tools-unresolved`.

### DR-598. BoundaryFill Boolean targets and control words

**Question.** Which bodies receive the selected cell volumes for Join, Cut, and Intersect? What effects do additional operation words specify?

**Known.** Operation words 1 through 4 specify Join, Cut, Intersect, and New Body respectively.

**Need.** A stored target relationship for the three Boolean operations and controlled saves for any additional operation words.

**Note.** The effects project with `feature.boundary-fill-targets-unresolved` when they require existing bodies. An unassigned operation word reports `feature.boundary-fill-operation-unresolved`.

### DR-599. BoundaryFill auxiliary references

**Question.** What roles do the settings fields, remaining prefix bytes, and named-tail references carry?

**Known.** The selected-cell prefix contains a version-zero settings reference. The removal form contains two counted lists. The named scope tail supports associated references and optional middle and closing references.

**Need.** Controlled saves that vary the auxiliary references and settings while keeping the selected cells and tools fixed.

### DR-819. Opaque word in a typed one-sided distance Extrude

**Question.** What does a nonzero opaque distance word encode in the direct typed-reference scope?

**Known.** Scopes with this word store a solid operation, a profile-plane start, and fixed distance and taper carriers. A Join transition inserts an end cap at the stored distance. A Cut transition opens the material between the start plane and the existing end cap within the stored travel.

**Need.** Paired saves that vary this word and identify the Extrude setting or record that supplies it.

**Note.** The decoder retains the word and its offset. Neutral projection uses the stored blind distance and reports `feature.extrude-opaque-distance-word-assumed`.

### DR-880. Typed timeline-owned Base Feature body pairing

**Question.** How do the typed body records' passive-reference properties bind the parameter-body and result-body runs when their targets differ from the run positions?

**Known.** A one-body typed frame has a `BASEFEAT_PM_BODY_REF` property on the first body record that names the second body record and a `BASEFEAT_DM_BODY_REF` property on the second record that names the first. The reader pairs the two counted runs by position and uses the first entry's width for both runs.

**Need.** Paired saves with different property targets or entry widths, and the corresponding final body geometry.

**Note.** The decoder retains positional pairing with `feature.owned-body-pairing-assumed` for the typed body runs.

### DR-820. Region member lists across a crossing curve that leaves the arrangement

**Question.** Which stored relation selects one member per fragment or one member per curve run at an interior crossing whose other curve leaves the arrangement?

**Known.** A counted list in a planar Extrude can name five curves once each. A non-construction line crosses one listed curve inside the selected face. Both arms of the crossing line leave the cycle graph. The final B-rep agrees with the face bounded by the five named curves. The decoder retains exact fragment matches and reads a unique boundary run across a pruned transverse interior crossing with `feature.profile-crossing-runs-assumed`. `f3d.md` §3.1 "The member list of a counted region names a neutral profile" makes every point where two curves meet a node, and a walk counts a curve once for each fragment. A line that ends on a rectangle side splits that side into two fragments, and the member list names the side twice. In the arrangement of the five-curve list, a non-construction line crosses one of these curves inside the selected face; both parts of the crossing line leave the arrangement. The walk of the face holds the crossed curve as two fragments, so the list matches no walk. The final B-rep of the Extrude matches the face that the five curves bound.

**Need.** Paired saves with the same profile and a changed crossing-curve endpoint, tangent, or cycle connection, with the corresponding counted member lists.

**Conflict.** None.

**Note.** Endpoint splits retain their fragment multiplicities.

### DR-890. Tail marker of the two-opaque-byte planar curve tail

**Question.** Which byte of the three bytes between the zero pad and the u32 lane is the tail marker when a planar sketch-curve tail stores two opaque bytes?

**Known.** `f3d.md` §3.1 "Immediately after the 96-byte analytic payload" gives the three-opaque-byte tail, where the tail marker directly precedes the u32 `1`, and the two-opaque-byte tail, where one byte follows the two opaque bytes and precedes the u32 lane. The decoder reads that byte as the tail marker and charges `sketch.curve-tail-marker-assumed` for every two-opaque-byte tail.

**Need.** A two-opaque-byte tail with a nonzero byte before the u32 lane, and the counted region member lists that name or omit that curve.

### DR-898. Construction flag of the two-opaque-byte planar curve tail

**Question.** Does a u32 lane of `1` in a two-opaque-byte planar sketch-curve tail mark construction geometry?

**Known.** In the three-opaque-byte tail the u32 after the tail marker is `1`, and the byte after it is the construction flag. In the two-opaque-byte tail the byte after the u32 lane is always `1`, and the lane is `0` or `1`. The decoder reads a lane of `1` as construction geometry and charges `sketch.curve-tail-construction-assumed` for every two-opaque-byte tail.

**Need.** Two-opaque-byte tails of curves with a known construction status, for lane `0` and for lane `1`.

### DR-895. Direction record of an inactive rectangular-pattern direction one

**Question.** Does a rectangular-pattern scope whose direction one has zero distance or count one, and whose pattern payload stores a V displacement, store a direction record for direction one, and does the payload's U-direction reference at offset 122 name that record?

**Known.** `f3d.md` §3.1 states that a direction with count greater than one and zero distance repeats the seed in place, that a scope can store both records when one direction has count one, and that the payload names the U-direction record at offset 122 and stores the V displacement at offset 63.

**Need.** A save with a zero-distance direction one beside a pattern payload, with its direction records and instance positions.

**Note.** The decoder reads the record that the payload names as the direction-one record, takes direction two from the payload V displacement, and charges `feature.rectangular-pattern-direction-one-record-assumed`. A payload without a V displacement leaves the one record to direction two.

### DR-892. Packed-direction split component flag `Z`

**Question.** What does the `Z: true` key in the descriptor of a packed-direction split component encode?

**Known.** The flagged components inflate to their declared byte counts, the two components have equal lengths divisible by four, and every value read as f32 is finite and lies in [-1, 1]. A flagged corner-normal stream decoded with the `{T: 3}` layout matches the triangle face normals with mean dot product 0.997; the mirror component order gives -0.997.

**Need.** Paired containers of one mesh saved with and without the flag.

**Note.** The decoder reads the flagged payload with the `{T: 3}` layout, retains the flag on the native split-stream record, and reports `mesh.component-flag-uninterpreted`.

### DR-1029. Version-3 settings reference prefix

**Question.** What does the leading reference of the version-3 sheet-metal settings frame name, and what control does it supply?

**Known.** `f3d.md` §3.1 gives the frame. The reference is a present local reference that resolves in the same primary record index.

**Need.** Controlled saves must vary the record that the reference names. Their typed links and final sheet geometry must establish its target type and role.

**Conflict.** None.

**Note.** The native settings record keeps the reference; the decoder does not read it as a control. The edge-flange radius derivation uses only frames with mask zero.

### DR-889. Extrude selection members in sketch text

**Question.** Which glyph region of a sketch text does each Extrude selection member name?

**Known.** `f3d.md` "An Extrude selection resolves to neutral sketch-loop indices" defines the member resolution through Sketch points, curves, and history states. Each member of one selection group names a distinct profile region of the selected Sketch. The local identity of a member that picks a glyph region matches no Sketch point or curve identity, and the `txt_tag` and `textex_tag` records store no glyph outline. A group can hold more members than the regions that the Sketch curves bound.

**Need.** Saves that pick a subset of the glyphs of one text, and saves that pick a glyph counter, with the local identities of each member.

**Note.** When the distinct members of the one selection group of an Extrude match no Sketch point, Sketch curve, or history binding and outnumber the atomic regions of the line, arc, and circle curves of a Sketch with non-construction text, the decoder takes the excess members as glyph picks. When the member count equals the count of non-whitespace characters of the Sketch text, the profile names the text entities and the decoder reports `feature.profile-text-glyphs-assumed`. Otherwise a Sketch with one profile loop selects that loop, any other Sketch keeps the group as a native selection within the Sketch, and the decoder reports `feature.profile-text-glyphs-native`. A Sketch with text and exactly one profile loop resolves to that loop when the distinct members are at most its atomic regions.

### DR-920. Scene slot of a tessellated mesh body

**Question.** What does the Scene-node slot of a `MeshTessellate` produced body name, and which Scene node displays the body?

**Known.** The slot names a record of type `4912A0EC-8A3D-4FE2-9F0F-A5C2CB493A07`, base-type GUID `804A1AC8-DCA8-4204-AED5-1E64BE8B7CAB`, type version `0`, and add-in name `Scene`. Each record is 95 bytes: its indexed header, 34 zero bytes, byte `01`, six f64 values, and byte `01`, the form of the Scene-state record. The six values are the maximum and minimum corners of the body in centimetres. The Scene nodes of type `702B9CD2-537C-429E-8CC4-22BEEEB98C37` are as many as the source bodies. Fusion 2706.1.17 saved the produced bodies in this form.

**Need.** A mesh import saved by Fusion 2706.1.17, and a `MeshTessellate` feature saved by an earlier Fusion version.

**Note.** The mesh graph requires a Scene node in that slot; such a design reports `mesh.container-undecoded`.
