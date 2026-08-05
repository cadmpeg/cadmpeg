# SolidWorks `.sldprt`: Open Items

This document lists the parts of the SolidWorks `.sldprt` format that we do not know. The specification `sldprt.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Body classification

### BC-01. Other class-root layouts

**Question.** What grammar binds a class-root vector after `index_map_offset` when the vector does not satisfy a defined body, shell, face-use, or cluster-key layout?

**Known.** `sldprt.md` §5 "Face records use these families:" through `sldprt.md` §6 "The disc22-disc12-face layout uses" define the disc-keyed ownership layouts and the class-number-independent cluster-key layout. Each complete layout assigns canonical faces to a stored body.

**Need.** We must know the other layouts to assign their faces to bodies.

### BC-02. Deltas faces outside partition intervals

**Question.** Which body owns a deltas-stream face that is outside all partition intervals of a multi-chain site?

**Known.** `sldprt.md` §6 "The disc04-root layout uses" defines interval ownership for the class-number-independent layout. A sole chain owns all canonical faces in its site.

**Need.** We must know the owner to construct the final body membership of a multi-chain site.

### BC-03. Superseded partition faces

**Question.** Which field or relation identifies the partition face that a deltas face supersedes?

**Known.** `sldprt.md` §4.2 "A deltas stream groups its records into change sets." states that change-roster membership does not identify persistence. `sldprt.md` §4.2 "A deltas change set can re-create a body's faces under new attributes." states that a full deltas bridge is part of the final state and can supersede a partition face.

**Need.** We must identify the superseded face to prevent duplicate faces in the final body.

## 2. Geometry carriers

### GC-01. Non-isoparametric B-spline trim UV

**Question.** How do we derive the UV curve for a non-isoparametric trim on a B-spline face?

**Known.** `sldprt.md` §7.1 "00 TT  [ff]?" through `sldprt.md` §7.2 "00 2d  marker [ff?]  value_count u32 BE  attr u16 BE  f64[value_count] BE   ; poles /" define exact pcurves for the supported analytic, isoparametric, polar-NURBS, and ruled-surface cases. The Parasolid stream does not store a two-dimensional UV control array.

**Need.** We must know the convention to construct the trim in the surface parameter space.

### GC-02. Missing intersection witnesses

**Question.** Where are the chart, terminator, and support-UV witnesses for an intersection composite when its referenced witnesses are absent or inconsistent?

**Known.** `sldprt.md` §7.2 "Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier," through `sldprt.md` §7.3 "An edge's `00 10.refs[3]` can point to either intersection carrier for a curve defined by the" define both intersection carriers and the three witness record families. The two support surfaces define the exact intersection only when the carrier selects a usable branch.

**Need.** We must find the witnesses to construct the bounded intersection curve.

### GC-03. Surface-owned edge curve attributes

**Question.** What grammar and role do untyped edge curve attributes have when a surface record owns them instead of a curve carrier?

**Known.** `sldprt.md` §7.1 "Stream-scope" defines `00 86` as a B-spline or list curve carrier. `sldprt.md` §7.2 "Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier," defines the intersection curve carriers. An edge selects its support curve through `00 10.refs[3]`.

**Need.** We must know the grammar to construct the edge curve.

### GC-04. Offset-surface carriers

**Question.** Which record carries offset-surface geometry, and what is its payload grammar?

**Known.** `sldprt.md` §6 "The disc1e-disc14-face layout uses one `0x1e/flo2` region with a slot-1 sentinel and the chain" through `sldprt.md` §7.3 "**`00 28` chart** — the solved point cache:" define compact analytic, B-spline, intersection, and constant-radius rolling-ball surface carriers.

**Need.** We must know the carrier to construct an exact offset surface.

### GC-05. Variable-radius blend carriers

**Question.** Which record carries non-constant-radius blend geometry, and what is its payload grammar?

**Known.** `sldprt.md` §7.4 "A `00 38` surface carrier defines a circular rolling-ball blend between two support surfaces:" defines the `00 38` constant-radius rolling-ball surface. Its two offsets have one common nonzero magnitude.

**Need.** We must know the other carrier to construct a variable-radius blend surface.

### GC-06. Surface-intersection surface carriers

**Question.** Which record carries surface-intersection surface geometry, and what is its payload grammar?

**Known.** `sldprt.md` §7.2 "Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier," defines curve carriers for the intersection of two surfaces. It does not define a surface carrier for this geometry family.

**Need.** We must know the carrier to construct the exact surface.

### GC-07. Spline-on-surface carriers

**Question.** Which record carries spline-on-surface surface geometry, and what is its payload grammar?

**Known.** `sldprt.md` §7.1 "Stream-scope" defines B-spline surface and curve carriers. It does not define the relation that makes a spline a curve on a support surface.

**Need.** We must know the carrier and relation to construct the exact surface geometry.

## 3. Container metadata

### CM-01. Cache-cell prefix

**Question.** What index-state information does the cache-cell prefix encode?

**Known.** `sldprt.md` §1.2 "The bytes before the first valid block and the long inter-block gaps hold a **fixed-cell" through `sldprt.md` §1.2 "A valid cache cell satisfies `two_L == 2L`, `half_L == L//2`, `0 < name_len < 500`, and has a" define the cache-cell grid, cell size, and section-index interpretation.

**Need.** We must know the prefix semantics to validate and write a cache cell.

### CM-02. Cache-cell fill

**Question.** What index-state information does the cache-cell fill encode?

**Known.** `sldprt.md` §1.2 "The bytes before the first valid block and the long inter-block gaps hold a **fixed-cell" through `sldprt.md` §1.2 "A valid cache cell satisfies `two_L == 2L`, `half_L == L//2`, `0 < name_len < 500`, and has a" define how nonzero cache-cell section indices address blocks.

**Need.** We must know the fill semantics to validate and write the grid.

### CM-03. Cache-cell `type_id` high half

**Question.** What does the high half of a cache-cell `type_id` encode?

**Known.** `sldprt.md` §1.2 "The bytes before the first valid block and the long inter-block gaps hold a **fixed-cell" through `sldprt.md` §1.2 "A valid cache cell satisfies `two_L == 2L`, `half_L == L//2`, `0 < name_len < 500`, and has a" define the cell `type_id` field and its relation to the addressed block.

**Need.** We must know the high-half semantics to validate and write the type identifier.

### CM-04. Tail-directory fill

**Question.** What index-state information does the variable-length fill after the final tail-directory entry encode?

**Known.** `sldprt.md` §1.3 "The file tail carries an **OPC package section directory**" through `sldprt.md` §1.3 "The file tail carries an **OPC package section directory**" define tail-directory entry framing and section lookup.

**Need.** We must know the fill semantics to validate and write the tail directory.

### CM-05. Other inline entity families

**Question.** What fixed slot count does each inline entity family outside the canonical face families use in each Parasolid schema?

**Known.** `sldprt.md` §5 "Top-level entity families:" defines the common entity header. `sldprt.md` §10 "Inline `00 51` subrecords use a fixed slot count" defines inline record boundaries when the schema-specific slot count is known.

**Need.** We must know each slot count to find record boundaries without treating payload bytes as delimiters.

### CM-06. Partition and deltas precedence

**Question.** Which record takes precedence when partition and deltas streams contain records with the same site, attribute, and sequence?

**Known.** `sldprt.md` §3.2 "An attribute id is **not** globally unique." defines the shared site namespace. `sldprt.md` §4.2 "A deltas stream groups its records into change sets." through `sldprt.md` §4.2 "A deltas change set can re-create a body's faces under new attributes." define deltas change sets and final-state faces, but do not define equal-key precedence.

**Need.** We must know the precedence to select one final record.

### CM-07. `moTransRefPlaneData_c` gap

**Question.** What does the byte run between the `moTransRefPlaneData_c` class token and the first of its nine f64 values encode, and what fixes its length?

**Known.** `sldprt.md` §8 "**Materials / metadata**" gives the field offsets of each document metadata record from the end of its class token. Every other record in that table starts its fields at token end +0. This one starts them after a gap. The decoder finds the value block by the first offset in `0..64` at which nine finite f64 values satisfy the extent constraints.

Observed gap:

| gap length | bytes | record that follows |
| --- | --- | --- |
| 8 | `ff ff ff ff ff ff ff ff` | plane center xyz |

**Need.** We must know the gap to write the record back. A writer that omits it moves every later record in the SW Objects payload, which moves the byte offset each `sldprt:metadata:` identifier carries, so a rewrite that changes nothing still renames those attributes.

## 4. Auxiliary lanes

### AL-01. DisplayLists face ranges

**Question.** How does a B-rep face attribute select its triangle range in a DisplayLists block?

**Known.** `sldprt.md` §7.3 "**`00 28` chart**" defines the DisplayLists descriptor table, strip lengths, and triangle-count relations. `sldprt.md` §5 "Face records use these families:" defines B-rep face identities.

**Need.** We must know the mapping to attach each tessellated triangle to its face.

## 5. Design intent

### DI-01. Other optional feature-manager node identities

**Question.** What native field identifies the role of each optional classless feature-manager node that does not satisfy a defined layout-scoped identity?

**Known.** `sldprt.md` §2 "An `moCurvePattern_c` feature-input object is immediately preceded by its seed feature object" through `sldprt.md` §2 "An `moLPattern_c` interval without a line-reference record carries each displayed translation" identify the annotations container, principal planes, model origin, lights-and-cameras container, ambient and directional lights, sheet-metal node, and exploded-views container. The identities depend on a complete native-class roster. Other source identifiers are allocation positions and are not role codes. `sldprt.md` §2 "A classless Keywords `Feature` whose `Type` token is `EquationDriven` is the equation" identifies the equation container by its operation-family token, which is a role code and needs neither a native class nor a reserved source identifier.

**Need.** We must distinguish the remaining binders, comments, body folders, materials, notes, sensors, favorites, history, selection sets, and markups.

### DI-02. Equation angular-unit mode

**Question.** Which native field stores the equation manager's angular-unit mode?

**Known.** `sldprt.md` §2 "A bare integer Keywords dimension bound to a unique driving angular scalar denotes milliradians" through `sldprt.md` §2 "A nonempty Keywords parameter value with no scalar literal, operator, grouping delimiter," define expression identifiers and unit-bearing literals. An explicit angular unit determines the interpretation of a trigonometric operand.

**Need.** We must know the document mode to evaluate a bare numeric trigonometric operand.

### DI-03. Default document properties

**Question.** Which native carrier stores the default document-property namespace used by equations?

**Known.** `sldprt.md` §2 "When exactly one line-distance operand identifies a profile line, the other operand identifies" through `sldprt.md` §2 "A bare `0` or `1` Keywords dimension bound to a unique driving distance" define configuration records, configuration-local properties, and equation evaluation. They do not bind a default document-property carrier.

**Need.** We must find the carrier to resolve an equation that references a default file property.

### DI-04. Configuration property lookup

**Question.** How does a `property@configuration@part` operand select its property namespace?

**Known.** `sldprt.md` §2 "When exactly one line-distance operand identifies a profile line, the other operand identifies" through `sldprt.md` §2 "A bare `0` or `1` Keywords dimension bound to a unique driving distance" define independently evaluated configuration snapshots and configuration-local properties.

**Need.** We must know the lookup rule to evaluate a configuration-qualified property operand.

### DI-05. Offset-edge relation invariant

**Question.** What neutral geometric invariant does an offset-edge marker relation impose?

**Known.** `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" through `sldprt.md` §2 "A primary line-or-circle geometry handle on a transformed line segment identifies that line" define operand resolution and the neutral invariants of the supported dimensional and geometric relations.

**Need.** We must know the invariant to project the native relation as a neutral sketch constraint.

### DI-06. Arc-cardinal relation invariants

**Question.** What neutral geometric invariant does each top, bottom, left, and right arc-cardinal marker relation impose?

**Known.** `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" through `sldprt.md` §2 "A primary line-or-circle geometry handle on a transformed line segment identifies that line" define operand resolution and the neutral invariants of the supported relation families.

**Need.** We must know the invariants to project these native relations as neutral sketch constraints.

### DI-07. Ambiguous relation-locus ownership

**Question.** Which profile locus does a relation operand select when its marker graph identifies a handle but does not identify one geometric locus?

**Known.** `sldprt.md` §2 "A Keywords configuration's decimal `id` attribute is the slot identity for" through `sldprt.md` §2 "Feature-input geometry-handle coordinates and the nested Parasolid profile differ by a signed" define handle chains, coordinate transforms, canonical shared-coordinate loci, and relation operand selection. Ambiguous chains and transform-dependent markers remain unresolved.

**Need.** We must select one locus to construct the neutral constraint.

### DI-08. Relation codes `29..32`, `36..41`, and `43..85`

**Question.** What neutral invariant and operand roles does each relation code in `29..32`, `36..41`, and `43..85` have?

**Known.** The native numeric taxonomy defines identities through code `85`. `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" through `sldprt.md` §2 "A primary line-or-circle geometry handle on a transformed line segment identifies that line" define the neutral semantics of the supported codes.

**Need.** We must know each invariant and operand family to project the remaining codes.

### DI-09. Relation codes above `85`

**Question.** What native relation does each code above `85` identify?

**Known.** The defined native numeric taxonomy ends at code `85`.

**Need.** We must know the identities before we can define their operands and neutral invariants.

### DI-10. Ambiguous marker transforms

**Question.** Which marker-to-profile transform applies when coordinate sets permit more than one signed-axis transform and the transforms select different loci?

**Known.** `sldprt.md` §2 "Keywords length literals use the suffixes `uin`, `mil`, `mm`, `cm`, `in`, `ft`, `nm`, `um`, `µm`" through `sldprt.md` §2 "Operand tags" define transform selection, placement fallback, and the case in which all valid transforms give the same locus set.

**Need.** We must select one transform to bind each remaining marker to profile geometry.

### DI-11. Ambiguous linked reference markers

**Question.** Which profile entity does a reference marker select when its linked loci do not identify one entity?

**Known.** `sldprt.md` §2 "A Keywords configuration's decimal `id` attribute is the slot identity for" and `sldprt.md` §2 "Keywords length literals use the suffixes `uin`, `mil`, `mm`, `cm`, `in`, `ft`, `nm`, `um`, `µm`" define unique linked-handle and shared-entity resolution.

**Need.** We must select one entity to bind the reference operand.

### DI-12. Omitted dimensioned circles

**Question.** Which native field marks dimensioned circular geometry as construction geometry when it is absent from the selected profile stream?

**Known.** `sldprt.md` §2 "A compact-legacy kind `2` bounded curve with locus `05 00 01 00` and the compact indexed" through `sldprt.md` §2 "An extended-prefix kind-`1` profile circle uses the same equal-index 104-byte or terminal" define ordinary and construction full-circle layouts. `sldprt.md` §2 "An `sgSlot_c` declaration may immediately precede a current-, legacy-, or extended-prefix slot record with" distinguishes aggregate slot descriptors from independent curve geometry.

**Need.** We must know the discriminator to prevent omitted construction circles from becoming profile geometry.

### DI-13. Marker-only profile placement

**Question.** How is a marker-only profile placed in model space when it has no local, contextual, or unique lane-wide compact reference-plane record?

**Known.** `sldprt.md` §2 "Point-reference object indices address sketch-marker local identifiers within the owning feature" defines model-space recovery from a planar sketch placement. `sldprt.md` §2 "An `moLPattern_c` feature-input object is immediately preceded by its seed feature object. That" through `sldprt.md` §2 "Among classless, parameterless, propertyless history records, `Feature` source ID `1` is the" define the supported reference-plane frames.

**Need.** We must know the placement to transfer the profile from planar coordinates to model coordinates.

### DI-14. Other Keywords operation families

**Question.** What neutral operation does each Keywords operation family outside the typed neutral feature set represent?

**Known.** `sldprt.md` §2 "A detailed curve record is immediately followed by a curve-detail marker of the same generation:" through `sldprt.md` §2 "A bare `0` or `1` Keywords dimension bound to a unique driving distance" define the native records and operands of the supported history operations.

**Need.** We must know the operation semantics to transfer the remaining design history.

### DI-15. Up-to-vertex endpoint selector

**Question.** Which endpoint does the u32 selector in an up-to-vertex code-`3` edge-endpoint reference select?

**Known.** The point-reference form retains the edge and endpoint selector. `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" states how ordinary point-reference object indices resolve.

**Need.** We must know the selector values to terminate the extrusion at the correct edge endpoint.

### DI-16. Point-reference codes above `9`

**Question.** What point-reference form does each code above `9` identify?

**Known.** Point-reference codes through `9` have defined native forms and neutral projections.

**Need.** We must know the forms to project their selected geometry.

### DI-17. Other second-direction termination codes

**Question.** What termination does each second-direction end-spec code other than `0` and `1` represent?

**Known.** `sldprt.md` §2 "A named feature-input object bound to a classless history `Sketch` record with a nonzero source" defines second-direction code `0` as absent and code `1` as through-all.

**Need.** We must know the other codes to construct the second extrusion extent.

### DI-18. Other first- and second-direction combinations

**Question.** What semantics apply when a nonzero second-direction code occurs with a first-direction code other than `0`, `1`, or `9`?

**Known.** `sldprt.md` §2 "A named feature-input object bound to a classless history `Sketch` record with a nonzero source" defines the supported one-direction and two-direction combinations.

**Need.** We must know the combination semantics to construct both extrusion extents.

### DI-19. Reference-bearing end-spec shapes

**Question.** What record uses the end-spec shape when the word at `+18` is a reference child instead of a termination code?

**Known.** `sldprt.md` §2 "A named feature-input object bound to a classless history `Sketch` record with a nonzero source" defines the end-spec anchor and the termination words at `+18` and `+22` for valid end-spec children.

**Need.** We must distinguish the other record family from an extrusion end specification.

### DI-20. Feature-local face reconciliation

**Question.** What is the disposition of a feature-local face identity when no surviving face carries that identity?

**Known.** `sldprt.md` §2 "An extrusion object-name record is followed by four zero bytes, a little-endian u16 family word," through `sldprt.md` §2 "When operation objects and their dimension children form separate ordered groups, a blind" define the `moSingleFaceRef_w` path forms and their feature-local face selection. `sldprt.md` §5 "Face records" defines how the terminal owner and feature-local face identity select the surviving face through `ATOM_ID_2001`.

**Need.** We must distinguish a face consumed by a later operation from a face whose owner or identity attribute was not decoded.

### DI-21. Other inline extrusion operation bytes

**Question.** What Boolean operation does each inline operation byte other than `moExtrusion_c` byte `00` and `moICE_c` byte `02` represent?

**Known.** `sldprt.md` §2 "Feature-tree" defines those two operation bytes and states that `moICE_c` byte `00` does not carry an operation.

**Need.** We must know the other byte values to construct the extrusion Boolean operation.

### DI-22. Other extrusion form codes

**Question.** What distinguishes a joining `moICE_c` form-`11` object from a subtracting form-`11` object, and what operation and record shape does each sparse extrusion object outside the defined form codes use?

**Known.** `sldprt.md` §2 "Keywords element order is serialization order, not regeneration order. Neutral regeneration" through `sldprt.md` §2 "Feature-tree" define direct, repeated-class, sentinel, terminated-trailer, and sparse-trailer objects. Most form-`11` objects subtract, but a minority join.

**Need.** We must know the discriminator and the other form codes to parse the object and construct its Boolean operation.

### DI-23. Combine-body reconciliation

**Question.** How does a body selected by a compact `moCombineBodies_c` target or tool path map to a body in the final B-rep?

**Known.** The compact paths identify generated feature-local bodies. `sldprt.md` §4.2 "00 01 00 01" through `sldprt.md` §6 "The disc22-disc12-face layout uses one `0x22/flo2` region with a slot-1 sentinel and the chain" define final B-rep body identities and ownership. `ATOM_ID_2001` carries face identities and does not bind a feature-local body identity.

**Need.** We must know the mapping to bind the Boolean target and tool bodies.

### DI-24. Delete-body reconciliation

**Question.** How does an `moDeleteBody_c` regeneration-input-local body identity map to a body in the final B-rep?

**Known.** The compact record retains the regeneration-input-local identity. `sldprt.md` §4.2 "00 01 00 01" through `sldprt.md` §6 "The disc22-disc12-face layout uses one `0x22/flo2` region with a slot-1 sentinel and the chain" define final B-rep body identities and ownership.

**Need.** We must know the mapping to delete the selected final body.

### DI-25. Component-edge path reconciliation

**Question.** How does an edge selected by an entry-form `moCompEdge_c` path map to an edge in the final B-rep?

**Known.** The entry path identifies a generated feature-local edge. `sldprt.md` §3.2 "An attribute" through `sldprt.md` §4 "**Bridge `00 0e`:** `refs[2]` = owning loop-head, `refs[4]` = primary surface carrier (compact" define final B-rep edge identity and direction. `ATOM_ID_2001` carries no edge identity. The `EDGE_PERM_ID_2_2003` attribute grammar is not defined.

**Need.** We must know the mapping to bind the selected operation edge.

### DI-26. Compact edge-identity vectors

**Question.** What grammar and identity namespace does a compact edge vector use?

**Known.** Entry-form `moCompEdge_c` paths have a defined component-path grammar. Compact vectors remain separate native records.

**Need.** We must know the grammar and namespace to bind their selected edges.

### DI-27. Component-surface-body reconciliation

**Question.** How does a face selected by an `moCompSurfaceBody_c` path map to a face in the final B-rep?

**Known.** The path identifies a generated feature-local face. `sldprt.md` §4.2 "Deltas streams re-encode records in prefixed/tripled forms (each ref stored as a `[hi][lo][01]`" defines final B-rep face identities.

**Need.** We must know the mapping to bind the selected surface-body faces.

### DI-28. Unbound general-curve references

**Question.** How does a general-curve-reference form select geometry when it has no component-profile source record and no immediately preceding unique profile feature?

**Known.** `sldprt.md` §2 "An extrusion object without `Profile` or `DissectableChildren` has an unresolved profile unless" defines ownership for a unique enclosed sweep profile. The supported general-curve-reference forms bind through a component-profile source or an adjacent profile feature.

**Need.** We must know the other binding rule to select sketch or B-rep geometry.

### DI-29. Unbound composite sweep profiles

**Question.** How does a composite sweep-profile form select its profile when it has no unique enclosed planar profile stream and no immediately following unique profile feature?

**Known.** `sldprt.md` §2 "An extrusion object without `Profile` or `DissectableChildren` has an unresolved profile unless" defines the enclosed planar profile rule. The supported composite form can also bind to an adjacent profile feature.

**Need.** We must know the other binding rule to construct the sweep profile.

### DI-30. Other compact sweep Boolean codes

**Question.** What Boolean operation does each compact sweep code other than join code `15` represent?

**Known.** Compact sweep code `15` joins the sweep result.

**Need.** We must know the other codes to construct the sweep result operation.

### DI-31. Last-body-modifying feature identity

**Question.** What identifier space does the `LAST_BODY_MODIFYING_FEATURE_ID` body attribute use?

**Known.** Its values are not native feature object identifiers.

**Need.** We must know the identifier space to bind a body to its last history feature.

### DI-32. Compact line-reference width

**Question.** What distinguishes the eight-scalar compact line-reference form from the nine-scalar form when both final-triple interpretations are unit vectors?

**Known.** Both forms contain scalar triples that can satisfy the unit-vector invariant.

**Need.** We must know the discriminator to parse the reference without choosing a width from geometric plausibility.
