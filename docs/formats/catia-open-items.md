# CATIA V5 `.CATPart`: Open Items

This document lists the parts of the CATIA V5 `.CATPart` format that we do not know. The specification `catia.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Container and roster

### CR-01. Non-surface outer alias rows

**Question.** What grammar and role do outer `01 00 04 00 <tag>` rows have when they do not satisfy the surface-alias production?

**Known.** `catia.md` §7.5 `alias_row` defines the complete outer surface-alias row. Each freeform surface tag has one matching alias row. Vertex tags do not use that alias.

**Need.** We must distinguish other row classes from surface aliases and vertex registrations.

### CR-02. Extent flags

**Question.** What does each bit of the extent `flags` word control?

**Known.** `catia.md` §3.4 "ds+0x54 : k extent structs" defines the extent directory and the position of `flags`. The decoder retains the word and reports it as `extent_flags`.

**Need.** We must know the bit assignments to validate an extent and to write its flags.

## 2. Design intent

### DI-01. Compact schema-program semantics

**Question.** What operation does each compact schema-program record represent?

**Known.** `catia.md` §7.1 "A legacy schema catalog opens with" defines the program boundary, identity run, schema text fields, role selectors, and typed relation fields. The decoder retains their exact framing and identities.

**Need.** We must know the record semantics to transfer the complete design program.

### DI-02. Unresolved schema-role selectors

**Question.** What field role does each unresolved schema-selector byte select?

**Known.** `catia.md` §7.1 "Within an identity interval, `<inclusive-length:u8> <name:utf8> <selector>`" defines the byte production and its target selector. A literal role name assigns its role directly. An unresolved selector byte does not.

**Need.** We must know the selected role to assign field semantics.

### DI-03. `CATFeatCont` and `CATPrtCont` relationship

**Question.** How does a `CATFeatCont` object graph relate to the design history in `CATPrtCont`?

**Known.** `catia.md` §7.3 "An object graph is preceded by" defines object-graph framing, entity identity, structural owner groups, and source-schema selection. Container class names and owner groups are independent incidences.

**Need.** We must know the relationship to combine feature records into one design history.

### DI-04. Inline `7C09` reference roles

**Question.** What does each reference in an inline `7C09` body select?

**Known.** `catia.md` §7.3 "The fixed bytes in the inline production are structural" defines the inline body boundary and retains each reference identity.

**Need.** We must know the reference roles to bind objects and fields.

### DI-05. Inline `7C09` fixed-width words

**Question.** What does each fixed-width word in an inline `7C09` body control?

**Known.** The decoder retains the words inside the bounded inline body.

**Need.** We must know their roles to interpret and write the body.

### DI-06. Inline `7C09` control bytes

**Question.** What does each control byte in an inline `7C09` body control?

**Known.** The decoder retains the control bytes inside the bounded inline body.

**Need.** We must know their roles to validate and write the body.

### DI-07. Compound relation-program result binding

**Question.** Which compound relation-program frame slot selects the `paramout` result entity?

**Known.** `catia.md` §7.3 "A compact compound relation-program instance is" and `catia.md` §7.3 "A separator-form compound relation-program instance is" defines lead-`12` and lead-`54` relation-program frames. A complete typed expression and its source symbols give the ordered inputs. The program identity, repeated reference, lead-`12` `ref(h)` context identity, and lead-`54` trailing identity remain distinct incidences. A selected entity class does not by itself assign a result.

**Need.** We must know the result slot to transfer a relation with an output.

### DI-08. Legacy relation `body` selector

**Question.** What identity space does a nonlocal `body` selector on a legacy typed relation use?

**Known.** `catia.md` §7.1 "A typed relation consists of" defines the local identity case. A `body` selector equal to the containing identity supplies the local relation context.

**Need.** We must resolve a nonlocal selector to bind the relation context.

### DI-09. Legacy relation `param` selector

**Question.** What identity space does a nonlocal `param` selector on a legacy typed relation use?

**Known.** `catia.md` §7.1 "A typed relation consists of" defines the local identity case. A `param` selector inside the containing run can select the relation parameter.

**Need.** We must resolve a nonlocal selector to bind the relation result.

### DI-10. Evaluated `String` relation inputs

**Question.** Which string-value entity supplies each typed `String` relation input?

**Known.** `catia.md` §7.1 "A legacy string-value packet is" defines named and evaluated string-value packets. A complete relation program can bind ordered source symbols.

**Need.** We must join the source symbol to its string packet to evaluate the relation.

### DI-11. Evaluated `String` relation results

**Question.** Which string-value entity stores the result of a typed `String` relation?

**Known.** `catia.md` §7.1 "A typed relation consists of" and `catia.md` §7.1 "A legacy string-value packet is" defines string packets and typed relation result signatures. A self-`body` relation whose `param` selector resolves one same-run identity selects that identity. For a zero-input `VoidType` relation, the exact output-assignment rule in `catia.md` §7.1 transfers that selected string packet when its value agrees with the right-hand result. A relation with input clauses or a nonlocal selector still does not establish its result packet.

**Need.** We must know the result entity for input-bearing relations and relations with nonlocal selectors so we can transfer the evaluated value.

### DI-12. Typed `Boolean` value production

**Question.** What byte production stores the scalar value of a typed `Boolean` parameter?

**Known.** `catia.md` §7.3 "A complete entity-record suffix value begins" defines scalar, unset, atom, control, and schema-selected object values. Boolean-named field classes can contain compound object payloads.

**Need.** We must know the scalar production to transfer a Boolean parameter.

### DI-13. Active configuration state

**Question.** Which field selects the active configuration state?

**Known.** `catia.md` §7.3 "A self-defining configuration record is" and `catia.md` §7.3 "A configuration-row link is" defines `Configuration` records, `configrow` successor chains, selected value schemas, and the source-ordered open intervals between rows. These incidences do not assign active state.

**Need.** We must know the selector to transfer the active configuration.

### DI-14. Configuration row semantics

**Question.** What does each entity in an open `configrow`-to-successor interval represent?

**Known.** Complete successor chains fix row order. The decoder retains each intervening entity in source order.

**Need.** We must know these roles to assign row names, parameter overrides, body membership, and feature replay order.

### DI-15. Sketch instance binding

**Question.** How do numeric tuples and the `2DPoint` and `PRTSketch` schema fields bind to the geometry of one sketch instance?

**Known.** `catia.md` §7.3 "An object graph is preceded by" and `catia.md` §7.3 "All `7C09` records in one graph carrying the same `owner_ref`" defines field framing, object identity, schema selection, and structural ownership. A separator-form owner declaration whose resolved class is `Sketch` establishes the sketch identity. A `PRTSketch` or `Sketch` field class alone does not assign geometry to that identity.

**Need.** We must know the binding to make a neutral sketch with geometry, placement, and profiles.

### DI-16. Sketch coordinate semantics

**Question.** Which numeric tuple components give the coordinates of a `2DPoint`?

**Known.** The decoder retains complete two-scalar numeric values and their selected field schemas.

**Need.** We must know the component order and units to transfer sketch points.

### DI-17. `PRTSketch` and `Sketch` payload roles

**Question.** What does each atom, list, and reference in a `PRTSketch` or `Sketch` field represent?

**Known.** The object graph retains these payloads, their schemas, references, and owner groups.

**Need.** We must know the roles to transfer sketch membership and geometry.

### DI-18. Constraint-range owner

**Question.** Which individual constraint owns each complete `Range`/`CstAttr_Dimension` or `Range`/`ComplexCst` value?

**Known.** `catia.md` §7.3 "A lead-`2` constraint-range entity has exactly two value selectors" defines both range forms and retains incoming payload references and object-head storage selectors as distinct incidences. `ListAggregator` references can include unrelated and repeated identities. A range transfers as one opaque sketch constraint only when exactly one total incoming incidence resolves to its same-graph paired source entity and object record, and that source object's complete owner chain reaches one transferred `Sketch` before another transferred feature. The source object record is retained as one unresolved native operand; neutral sketch entities, loci, parameters, and dimensional roles remain unresolved.

**Need.** We must know the owner to assign a range to a neutral constraint.

### DI-19. Sketch placement

**Question.** Which fields give a constructed sketch placement frame? Which fields give a support-face sketch placement frame?

**Known.** Object identity, field schema, references, and structural ownership are retained.

**Need.** We must know the frame to place sketch geometry in model space.

### DI-20. Sketch construction state and profile membership

**Question.** Which fields mark construction geometry? Which relation assigns sketch geometry to a profile?

**Known.** The decoder retains sketch-related objects and their exact incidences.

**Need.** We must know these semantics to separate construction geometry from closed profiles.

### DI-21. Sketch geometry classes

**Question.** What neutral geometry does each unresolved sketch-geometry class represent?

**Known.** The source-schema catalog gives each selected class name. A class name alone does not give the instance grammar.

**Need.** We must know the instance grammar to transfer the geometry.

### DI-22. Sketch constraints

**Question.** What are the operand and value fields of each dimensional and non-dimensional sketch constraint?

**Known.** Constraint-range values and structural incidences are retained separately.

**Need.** We must know the fields to transfer and solve the constraints.

### DI-23. Feature-instance grammar

**Question.** How do operation fields, definition-bound values, and structurally owned operand objects form one feature instance?

**Known.** `catia.md` §7.3 "All `7C09` records in one graph carrying the same `owner_ref`" defines each incidence independently. A complete two-definition value chain with a supported second role transfers one typed parameter, but it does not assign that parameter to an operation role. An operation-named field class or field vocabulary does not assign feature identity, operands, outputs, or replay order. An exact separator-form owner declaration for an admitted operation class, with matching class entry, owner entity, and structural owner, establishes one opaque feature identity and its source order; it does not assign the operation's semantic inputs.

**Need.** We must know the operation-specific binding that transfers profiles, directions, extents, outputs, and regeneration dependencies for each admitted feature family.

## 3. Standard nested `V5_CFV2`

### SN-01. `a5 03 32` header type codes

**Question.** What does each header token `05`, `09`, `0d`, and `1d` select?

**Known.** `catia.md` §6.2 "Frames an explicit rolling-ball surface jet." admits these four width-1 tokens for an `a5 03 32` rolling-ball jet. All four use the defined degree-5 jet grammar.

**Need.** We must know the selection rule to write the correct token.

### SN-02. `a5 03 32` numeric continuation

**Question.** What fields follow the three aligned jet blocks in each numeric-continuation length class?

**Known.** `catia.md` §6.2 "Frames an explicit rolling-ball surface jet." defines the knots and the value, first-derivative, and second-derivative blocks. The continuation has more than one length class.

**Need.** We must know its lanes and terminal fields to read and write the complete record.

**Note.** The enclosing frame closes the continuation after the three aligned jet blocks; its fields remain unresolved. The decoder transfers the complete known jet without imposing a fixed continuation-size limit. The `a8` parser instead requires the 59-byte tail that the specification states.

### SN-03. Width-coded class-`0x5e` header token

**Question.** What does the width-coded header token of a `b2`, `b3`, or `b4 03 5e` record select?

**Known.** The record framing gives the token width and value.

**Need.** We must know the selector namespace to interpret and write the edge record.

### SN-04. Class-`0x5e` terminal byte

**Question.** What does the terminal byte of a `b2`, `b3`, or `b4 03 5e` record control?

**Known.** The decoder retains the byte after the framed edge payload.

**Need.** We must know its role to validate and write the record.

### SN-05. Class-`0x18` descriptors

**Question.** What does each field of a class-`0x18` descriptor represent?

**Known.** `catia.md` §6.4 `b2/b3/b4 03 18` defines the record family and its framing.

**Need.** We must know the fields to interpret the descriptor.

### SN-06. Analytic-circle class-`0x23` definition

**Question.** What are the operands and the roles of the eight scalar lanes in an analytic-circle class-`0x23` edge definition?

**Known.** `catia.md` §6.3 "Class-`23` and class-`24` scalar edge definitions have payload" defines the class-`0x23` wrapper role in an exact pcurve dependency chain.

**Need.** We must know the direct definition to read a circle without an independent identity binding.

### SN-07. Standalone class-`0x24` record

**Question.** What does each field of a standalone class-`0x24` record represent?

**Known.** The decoder retains the framed record and its scalar lanes.

**Need.** We must know the fields to transfer its geometry.

### SN-08. Class-`0x25` scalar lanes

**Question.** What does each scalar lane of a class-`0x25` record represent?

**Known.** `catia.md` §6.7 "`a8 03 25` (extrusion directrix):" defines the `a8 03 25` directrix interval and fit tolerance independently of its sampled cache.

**Need.** We must know the lanes to decode the sampled cache.

### SN-09. `a8 03 25` sampled-cache coding

**Question.** How does the sampled-cache lane of an `a8 03 25` extrusion directrix encode its samples?

**Known.** Its references, solved parameter interval, and fit tolerance are defined.

**Need.** We must know the coding to reconstruct and write the cached curve.

### SN-11. Standard 3D spline cache

**Question.** How does the separate standard 3D spline cache encode its poles and knots?

**Known.** `catia.md` §6.3 "A complete consolidated edge run is" defines exact two-surface constructions from class-`0x20` pcurve jets, their supports, and their shared parameter interval. The separate 3D cache remains an opaque carrier.

**Need.** We must know the cache program to read a spline when the exact construction is unavailable and to write the cache.

### SN-12. Consolidated persistent-tag namespace

**Question.** How does an `op1` or absolute persistent CGM tag select a serialized record outside the exact class-`0x19` analytic-circle binding?

**Known.** `catia.md` §6.3 "When the `op1` support identity equals" defines the exact unique class-`0x19` binding. The decoder retains other persistent identities.

**Need.** We must resolve the namespace to bind other consolidated curve and support records.

### SN-14. Multiple FBB face groups

**Question.** Which fields assign standard-path faces and edges to topology components when the file has multiple separate FBB face groups?

**Known.** `catia.md` §5.1 "trim_record[i] -> face_outer_bound_row[i] -> face i" defines topology reconstruction for one positional spine and its FBB incidences.

**Need.** We must know cross-group membership to build all bodies and shells.

### SN-15. Standard arc branch without a witness

**Question.** Which arc branch applies when no adjacent face witness and no exact two-support object-stream pcurve witness is available?

**Known.** `catia.md` §5.6 "**Circle/arc endpoints by support intersection:**" defines arc selection when one of these witnesses fixes the branch.

**Need.** We must know the remaining selector to reconstruct the arc.

### SN-16. Class-`0x20` persistent reference

**Question.** How does the `op1` or persistent-tag reference in an `a5 03 20` record select a serialized record?

**Known.** `catia.md` §6.3 "`b2 03 20` is the B-family form" defines the pcurve payload and support binding for exact identity and chart matches.

**Need.** We must resolve other references to bind the pcurve support.

### SN-17. Class-`0x62` owner bounds

**Question.** Which coordinate system and axes do the binary64 box and three binary32 bounds in a fixed `b2`, `b3`, or `b4 03 62` owner tail use?

**Known.** The decoder separates the fixed five-byte header, binary64 box, and binary32 bounds.

**Need.** We must know the coordinate roles to interpret the bounds.

### SN-18. Class-`0x62` owner-to-face binding

**Question.** Which field binds a fixed `b2`, `b3`, or `b4 03 62` owner packet to its face record?

**Known.** Allocation links bind class-`0x5f` records to class-`0x62` owners.

**Need.** We must know the face binding to assign the owner metadata.

### SN-19. Cone `pre_range_scalar`

**Question.** What does `pre_range_scalar` control in a `b2 03 29` or `b5 03 29` cone record?

**Known.** `catia.md` §5.12 "The 184-byte payload is" defines the active angular range that follows this scalar.

**Need.** We must know its role to preserve the cone chart semantics.

### SN-20. Class-`0x18` parameter-point selectors

**Question.** What does each of the four prefix selectors in a `b2`, `b3`, or `b4 03 18` parameter-point record select?

**Known.** The decoder retains the four selectors and the parameter-point payload.

**Need.** We must know the selectors to bind the point to its carrier and parameter role.

### SN-21. `b2 03 3b` chart program

**Question.** What does each reference and control in a `b2 03 3b` cone-face chart program represent?

**Known.** `catia.md` §6.4 "**`b2/b3/b4 03 3b`** has width-coded header token" defines its bounded support-construction record.

**Need.** We must know the program to bind and evaluate the chart.

### SN-22. Class-`0x60` group types

**Question.** What does group type `2` select? What does each group type from `12` through `21` select?

**Known.** `catia.md` §6.5 `b2 03 60` defines type `3` as a cylinder chain.

**Need.** We must know the other type namespaces to parse their groups.

### SN-23. Counted class-`0x61` fields

**Question.** What does each counted reference and tail field of a `b2`, `b3`, or `b4 03 61` record represent?

**Known.** The decoder retains the structurally typed count, references, and tail.

**Need.** We must know the roles to bind the record to topology and geometry.

### SN-24. Long-form class-`0x61` fields

**Question.** What do the prefix, monotone members, five persistent references, and scalar of a long-form class-`0x61` record represent?

**Known.** The decoder retains each field in source order.

**Need.** We must know the roles to interpret and write the long form.

### SN-25. Class-`0x5f` owner role

**Question.** What higher-level object does each allocation-linked `b2`, `b3`, or `b4 03 5f` to `0x62` owner represent?

**Known.** The allocation link and owner packet are defined independently.

**Need.** We must know the object role to assign the owner to a feature or face.

### SN-26. Revolution profile allocation identity

**Question.** How does the `u16le` profile allocation identity in a `b2 03 2d` revolution record select its native profile record?

**Known.** `catia.md` §5.15 "The `00 33 30` byte is only" defines the axis frame, angular chart, and exact unique profile-interval binding.

**Need.** We must resolve the directrix identity when the interval binding is not unique.

### SN-27. Consolidated class-`0x27` plane carrier

**Question.** What are the fields of a `b2`, `b3`, or `b4 03 27` record, and which of them does its second payload byte select?

**Known.** The payload is that byte pair followed by a whole number of `f64le` values. The count changes with the second byte: `e4` gives 7 values, `c4` gives 8, and `ec` gives 6. A decoded value group holds an in-plane point, an in-plane unit direction with one component absent, and a trailing triple. No decoder reads the class.

**Need.** A consolidated edge side whose carrier is one of these records has no bound support. `catia.md` §6.3 "A resolved edge block binds" then recovers the side's chart relation to a standard plane face from the block's shared 3D loci, which needs that face to exist. A side with no standard face keeps no pcurve.

## 4. Object stream

### OS-01. Multi-surface class-`0x5f` face

**Question.** How does a `b5 03 5f` face select and combine more than one surface record?

**Known.** `catia.md` §6.7 "**Object-stream topology:**" defines single-carrier object-stream face, loop, edge, and vertex incidence.

**Need.** We must know the multi-surface rule to transfer the face.

### OS-02. Class-`0x5f` terminal controls

**Question.** What is the semantic difference between terminal controls `03` and `05` in a `b5 03 5f` face?

**Known.** Both values terminate the same framed face record.

**Need.** We must know the difference to validate and write the face.

### OS-03. Object-stream face normal sense

**Question.** Which field or relation gives a `b5 03 5f` face normal sense relative to its surface frame?

**Known.** Closed endpoint chains fix coedge traversal. They do not fix this face-level sign.

**Need.** We must know the sign to orient the neutral face.

**Note.** `b5::transfer::faces` writes `Sense::Forward` for every object-stream face and leaves the `sense` field at the face entity's `Inferred` exactness. The sign remains unresolved; the transfer loss note names the gauge.

### OS-04. Object-stream body kind

**Question.** Which source field gives the object-stream body kind?

**Known.** One-body ownership and incidence give a stable topology gauge. They do not identify the source field.

**Need.** We must know the field to preserve the native body classification.

### OS-05. Object-stream outward-shell sign

**Question.** Which source field gives the outward-shell sign?

**Known.** `catia.md` §6.7 "**Object-stream topology:**" defines the radial orientation equations used to select a consistent neutral shell gauge.

**Need.** We must know the source sign to preserve native shell orientation.

### OS-06. Object-stream edge terminal controls

**Question.** What is the semantic difference among edge terminal controls `01`, `02`, `21`, `22`, `25`, `26`, `29`, and `2a`?

**Known.** `catia.md` §6.7 "`b5 03 5e` (edge node):" defines the common edge record and its topology incidences.

**Need.** We must know the namespace to validate and write an edge.

### OS-07. Class-`0x62` secondary framing control

**Question.** What does the class-`0x62` secondary framing control select?

**Known.** The decoder retains it independently of the length-framed loop-node payload.

**Need.** We must know its role to parse other framing branches.

### OS-08. Class-`0x62` extended-metadata control

**Question.** What does the odd extended-metadata control in a class-`0x62` record select?

**Known.** The decoder retains the control and its bounded metadata.

**Need.** We must know its role to interpret and write the metadata.

### OS-09. Vertex-incidence terminal controls

**Question.** What is the semantic difference between terminal controls `00` and `04` in an object-stream vertex-incidence record?

**Known.** Both forms retain the same resolved vertex incidence.

**Need.** We must know the difference to validate and write the record.

### OS-10. Exact pcurve suffix scalar

**Question.** What does the positive scalar in the exact `b5 03 21` pcurve suffix control?

**Known.** `catia.md` §6.7 "`b5 03 21` (pcurve):" defines the pcurve geometry and parameter interval independently of this scalar.

**Need.** We must know the role to make the complete pcurve record.

### OS-11. Class-`0x2c` auxiliary scalars

**Question.** What does each decreasing auxiliary scalar in class-`0x2c` terminal forms `01 09` and `01 15` control?

**Known.** The enclosing class-`0x30` construction supplies the exact result chart.

**Need.** We must know the scalar roles to preserve the native construction.

### OS-12. Class-`0x37` support construction

**Question.** What operation does a `b5 03 37` support-bound construction represent? What does each of its six control bytes control?

**Known.** `catia.md` §6.7 "`b5 03 37` (support-bound surface construction):" defines the support and result-carrier references.

**Need.** We must know the operation and controls to construct the result surface directly.

### OS-13. Class-`0x3b` support construction

**Question.** What operation does a `b5 03 3b` support-bound construction represent? What do its first scalar and six controls mean?

**Known.** `catia.md` §6.7 "`b5 03 3b` (two-scalar support-bound surface construction):" defines the bounded record and support references.

**Need.** We must know the operation and fields to construct the result surface directly.

### OS-14. Class-`0x30` carrier kind `0x11`

**Question.** What construction does carrier kind `0x11` select in a `b5 03 30` record?

**Known.** Its result reference can carry cone geometry. The referenced cones do not satisfy the analytic parallel-offset distance equation.

**Need.** We must know the construction to transfer the result without using an incorrect cone offset.

### OS-15. Class-`0x34` elided pole program

**Question.** What does the fixed 141-byte program of an `a8 03 34` freeform surface encode?

**Known.** `catia.md` §6.1 "Payload: `degU` and `K_U`" and §6.7 "The elided-pole form places the fixed 141-byte" define the header lanes, the byte layout of the program, and the external pole allocation. The layout table gives a literal value or a shape test for each lane. It assigns a role to one lane only: the `f64le` at tail offset `+28` equals `last(V) - first(V)`. The specification names the program a "range/affine/extrapolation tail" and does not say which lanes hold the range, which hold the affine part, and which hold the extrapolation.

**Need.** We must know the lane roles to validate the program against the surface it belongs to, and to write it.

**Note.** This item was removed in the tree that added the layout table and the `valid_a8_elided_tail` validator, and it is restored here with a narrower question. A table of observed literal values is a shape test, not the semantics the question asks for. The pole location half of the original question is answered: `catia.md` §6.7 gives the external allocation, and `a5a8::records::a8_surface_from_external_grid` binds it. That binding is the subject of OS-21.

### OS-21. External pole grid binding

**Question.** Which field binds an external pole grid allocation to its elided-pole `a8 03 34` carrier?

**Known.** `catia.md` §6.6 "The elided-pole form places the fixed 141-byte" gives "Its external pole allocation is an unframed `nu×nv` XYZ grid ... occupying the complete gap between a length-closed `b5 <frame_flag> 21` pcurve and the next A/B-family frame", and gives "A grid binds only when its byte length, finite coordinate payload, and following frame boundary select one allocation." `a5a8::records::a8_surface_from_external_grid` tests every `b5 <flag> 21` frame in the complete stream, keeps every gap whose byte length equals the carrier's expected grid size and whose coordinates are finite, and requires exactly one such gap.

**Need.** The rule is a size match over the complete stream. It states no relation between the carrier and the pcurve that precedes the grid. Two carriers with equal pole counts make the same candidate set, so both withhold. We must know the binding field to resolve a carrier when the size is not unique.

## 5. Zero-entity `a9 03`

### ZE-01. Class-`0x5fxx` face terminal control

**Question.** What does the terminal control byte in each `0x5fxx` face record control?

**Known.** `catia.md` §8 "Record framing `a9 03 XX YY <payload[YY+8]>`, `record_length = YY + 12`; records reference each" defines the zero-entity face roster and support incidences.

**Need.** We must know the control to validate and write the face.

### ZE-02. Oriented-use allocation lane

**Question.** Which allocation-lane rule associates a `0638` oriented use with its owner-local `0x21xx` support and `0x05xx` incidence record?

**Known.** `catia.md` §8 "Record families:" defines these record populations and their owner-local identities independently.

**Need.** We must know the association to build neutral coedges.

### ZE-03. Physical-edge endpoint binding

**Question.** Which fields bind a `05 0b`, `05 10`, or `05 15` incidence allocation lane to its physical-edge endpoints?

**Known.** The decoder retains the incidence lanes and endpoint coordinate rows.

**Need.** We must know the binding to build neutral edges and vertices.

### ZE-04. `5e 1a` allocation namespaces

**Question.** What does each independent `T`, `X`, and `Y` allocation in the `5e 1a` tuple `[T,X,Y,T−1,T−2]` select?

**Known.** `catia.md` §8 "A `5e1a` edge-stride record contains" defines the tuple relation and retains all five identities.

**Need.** We must know the namespaces to bind the edge, supports, and incidences.

## 6. E5 `0D 03`

### E5-01. `0xa0` circle branch

**Question.** Which field selects the circle branch of an `0xa0` wrapper?

**Known.** `catia.md` §9 "Framing `E5 0D 03 <cls> <sub> <payload_size_u16le> 00 00 00 <record_id_u32le> <payload>`, stride" and `catia.md` §9 "Classes: `0x01` body" defines the admitted `0xa0` wrapper and analytic circle primitive.

**Need.** We must know the selector to choose the correct circle arc.

### E5-02. `0xa0` co-parametric mapping

**Question.** What is the general parameter mapping from an `0xa0` wrapper to its primitive?

**Known.** For the cone subset, `q_circle = (R/ca_q_scale) * q_ca`.

**Need.** We must know the other mappings to trim the primitive correctly.

### E5-03. Plane-cap digon orientation

**Question.** Which fields orient a plane-cap digon?

**Known.** `catia.md` §9 "**E5 orientation** is" defines the E5 incidence graph and the non-degenerate orientation equations.

**Need.** We must know the fields to orient the cap when its boundary is a digon.

### E5-05. Root orientation signs

**Question.** What does each of the two root `extra_orientation_signs` control?

**Known.** `catia.md` §9 "**E5 orientation** is" defines the other body, shell, face, and use orientation factors.

**Need.** We must know both roles to complete the source body and shell orientation equation.

### E5-06. Curve-support mode

**Question.** What does the mode byte after the pcurve reference lane in an E5 curve-support record select?

**Known.** The decoder retains the mode and the complete pcurve reference lane.

**Need.** We must know the mode to interpret the support relation.

### E5-07. Curve-support trailing bytes

**Question.** What fields occur after the fixed header of an E5 curve-support record?

**Known.** The decoder retains these bytes inside the bounded record.

**Need.** We must know the fields to read and write the complete support record.

### E5-08. Bound parameter code

**Question.** What does the trailing `u32` code after each E5 bound parameter control?

**Known.** `catia.md` §9 "**Topology:**" defines the bound parameter value and its edge incidence.

**Need.** We must know the code to interpret and write the bound.

### E5-09. Edge-use trailing fields

**Question.** What fields occur after the five counted references in an E5 edge-use record?

**Known.** The five references and the containing record boundary are defined.

**Need.** We must know the trailing fields to interpret and write the edge use.

### E5-10. Component orientation gauge

**Question.** Which field gives the global sign of a connected parity component?

**Known.** `catia.md` §9 "**E5 orientation** is" gives "its global sign follows majority `face_trailer_sign` alignment, with the first serialized loop as the stable gauge when the alignment count ties." That rule is a decoder procedure and names no field. Two decoded sign populations are unread: the loop trailer holds `3*edge_count+4` signed ternary words and only `ref_aligned_signs[1]` is used, and the root `0x08` tape holds one sign for each face and no code reads it.

**Need.** The sign reverses the cyclic member order of every loop in the component and toggles every member sense, which gives an inverted shell. The result stays radially coherent, so no gate rejects it, and the transfer loss note affirms that face and loop orientation transfer. We must know the field to fix the sign without a vote.

## 7. FBB-only and float-packed variants

### FV-01. `u24be` endpoint quotient binding

**Question.** How do native identities bind the logical quotient of `u24be` endpoints to the counted coordinate rows?

**Known.** `catia.md` §9 "E5 `05 08 01` coordinate rows occupy" defines the endpoint values and coordinate-row population. The topology solver can form the abstract endpoint quotient independently.

**Need.** We must know the native binding for byte-faithful vertex assignment.

### FV-02. Partial-spine family discriminator

**Question.** Which record discriminates the geometry family when an FBB-like run lacks a required edge or vertex population?

**Known.** `catia.md` §10 "A nested-`V5_CFV2` file with a valid FBB face group" defines the complete FBB-only partial-spine variant. An incomplete population does not satisfy that grammar.

**Need.** We must know the discriminator to select the correct decoder.

### FV-03. Partial-spine following-byte grammar

**Question.** What grammar follows the family discriminator of an incomplete FBB-like run?

**Known.** The complete-spine grammar does not assign these bytes.

**Need.** We must know the grammar to find record boundaries and geometry populations.

### FV-04. Variant loop-node payloads

**Question.** What payload grammar do loop nodes use outside the length-framed `b5 03 62` and `a8 03 62` forms?

**Known.** `catia.md` §10 "edge_table :=" defines both length-framed forms.

**Need.** We must know the other grammars to reconstruct variant loops.

### FV-05. Object-stream loop controls

**Question.** What does the object-stream loop control select?

**Known.** The float-packed decoder retains the control with its loop record.

**Need.** We must know its role to classify and write the loop.

### FV-06. Second signed edge control

**Question.** What does the second signed control for each float-packed edge control?

**Known.** The first direction relation and the ordered edge incidence are retained independently.

**Need.** We must know the second control to preserve edge-use semantics.

### FV-07. Optional loop metadata

**Question.** What does each of the ten optional numeric metadata fields in a float-packed loop represent?

**Known.** The decoder retains the fields in source order.

**Need.** We must know the roles to interpret and write the metadata.

### FV-08. Marker-only surface delimiter

**Question.** What delimiter grammar closes each record in the marker-only `00 33 3X` surface path?

**Known.** `catia.md` §11 "A nested-`V5_CFV2` file without a standard FBB spine" defines the admitted marker family. Marker bytes can also occur inside numeric payloads.

**Need.** We must know the delimiter grammar to separate adjacent surface records without a false marker match.

## 8. Appearance

### AP-01. `FeatureForColor` face selection

**Question.** How does `SelectingFeatureForColorUuid` select the face targeted by an `EC 03 R G B A` override that occurs with an `EB 01 R G B` all-face color?

**Known.** `catia.md` defines both color packets. The `EB` value applies to the complete face population. The `EC` value supplies the override color asset. The positional FBB rows independently store the effective face colors, so neutral face appearance binding does not depend on this UUID incidence. The application object graph contains `FeatureForColor` and `SelectingFeatureForColorUuid`, but the UUID-to-standard-face incidence is not assigned.

**Need.** We must know the incidence to preserve or write the native selection relation independently of the effective FBB presentation population.
