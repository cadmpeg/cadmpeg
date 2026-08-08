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

### CR-03. E5 record-stream location

**Question.** Which field selects the byte range that holds the E5 record stream?

**Known.** `catia.md` §3.3 "`FINJPL  ` (two trailing spaces) marks named stream blocks" gives a coherence rule: ten records that walk by their declared strides make a candidate, a coherent preamble wins, and storage type `0x0000008e` breaks equal-count ties. That rule is a decoder procedure. It names no field. `container::count_e5_records` searches forward for the next `e5 0d 03` marker and accepts a gap of any length before it, so the count is not a stride walk. The one validity test for each counted record is that a `u16le` size read at a fixed offset keeps the record in bounds.

**Need.** A coherent candidate makes `container::identify_variant` select `Variant::E5Stream`. That variant is applicable to one route, so the standard FBB route is not offered. We must know the field to select the stream without a count.

**Note.** `container::e5_record_stream_in_segments` takes the segment with the largest count through `max_by_key`, which keeps the last maximum. Two segments with equal counts and no `0x0000008e` type resolve by segment order. The specification sentence is a transcription of the function's doc comment.

### CR-04. Repeated BREP stream names

**Question.** Which field selects the `MainDataStream` and the `SurfacicReps` stream that make the BREP buffer when the directory holds more than one descriptor with that name?

**Known.** `catia.md` §3.4 "The descriptor names include" gives the descriptor names and the content of each stream. It gives no selection rule for a repeated name. `container::brep_stream` keeps the descriptor with the largest logical length in each class, and it accepts a second-class name that contains `Surf` instead of the complete name.

**Need.** The BREP buffer is the input to the variant census and to every standard-path decoder. We must know the selection to read the correct body.

### CR-05. Descriptor name position

**Question.** Which offset and length hold the stream name in a directory descriptor?

**Known.** `catia.md` §3.4 "A candidate is a descriptor when every extent validates" gives "The stream name is a UTF-16LE ASCII run in the descriptor header." `docs/layouts/catia.toml` holds no row for the name. `container::descriptor_name` takes the longest run of printable ASCII characters with `00` high bytes in the window from `ds-40` to `ds+0x50`, and it needs three characters as a minimum.

**Need.** The name selects the BREP streams (CR-04) and names the stream in the container report. An empty or wrong name makes `container::identify_variant` report `Variant::InnerNoDirectory` for a file that has a directory. We must know the offset to read a name that is not the longest run in that window.

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

**Known.** `catia.md` §7.1 "A typed relation consists of" and `catia.md` §7.1 "A legacy string-value packet is" defines string packets and typed relation result signatures. The type signature does not select the result packet.

**Need.** We must know the result entity to transfer the evaluated value.

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

### DI-24. Unresolved role selector framing

**Question.** Which condition selects the `80 <identity:u32le>` selector form of an unresolved role, and which selects the `<page:d1..e4> <low:u8>` form?

**Known.** `catia.md` §7.1 "Within an identity interval, `<inclusive-length:u8> <name:utf8> <selector>`" admits both forms. It gives no precedence. The two productions can match the same bytes: the first needs a nonzero byte at `field-6`, `80` at `field-5`, and a nonzero `u32le` at `field-4`; the second needs a nonzero byte at `field-3` and a byte from `d1` to `e4` at `field-2`. `legacy_entity::parse_role_selectors` tests the first form first and keeps it when it parses.

**Need.** The selected form gives the role offset and the target selector. The two forms give different offsets and different selectors for the same bytes. We must know the condition to bind the relation context and the relation result (DI-08, DI-09).

### DI-25. Text-field terminator before a role page

**Question.** Which condition closes a legacy text field with `FE`, and which condition continues it with a compound role, when the byte after the terminator is `E3`?

**Known.** `catia.md` §7.1 "A legacy string-value packet is" gives `FE` as the close of the nonempty single-value form, and gives `<role> E3 <selector-low:u8>` as the continuation of the compound form. It gives no rule for a terminated field that an `E3` byte follows.

**Need.** A role at a terminator offset moves the boundary of the field before it and stops the unresolved-role recovery of the opener after it. We must know the condition to keep one reading.

**Note.** The decoder holds both readings at the same time. `legacy_entity::parse_text_field` closes the field as `U8InclusiveLength`. `legacy_entity::parse_role_selectors` makes a role at the terminator offset, with the terminator byte `FE` as the role name. The two functions decide the same bytes differently.

### DI-26. `7C0A` payload `0x3c` form

**Question.** What does a `0x3c` byte introduce in a `7C0A` payload, and which field gives its extent?

**Known.** `catia.md` §7.3 "The fixed bytes in the inline production are structural" enumerates the assigned payload forms and gives "bytes outside those assigned forms remain literals; they do not create references." `0x3c` is not an assigned form. `object_graph::decode_payload` reads `3c <atom> <u32le>` as a bulk-table header when the `u32le` is not more than the payload byte length, and reads the `0x3c` byte as a literal atom when it is more.

**Need.** The two branches consume different byte counts, so the token boundary of every field after the `0x3c` byte changes. A different boundary makes or removes an object reference. We must know the extent field to walk the payload.

### DI-27. Feature-local parameter order

**Question.** Which field gives the position of a parameter in the parameter list of its feature?

**Known.** `catia.md` §7.3 "An object graph is preceded by" gives the byte offset of a design object's first field and the zero-based position of that object in the graph. It gives no order for the parameters of one feature. `design_feature` sorts the exact feature-owned parameters by object-record byte offset, then by entity byte offset, then by the identifier that the codec makes, and it publishes that rank as `DesignParameter.ordinal`.

**Need.** `DesignParameter.ordinal` states the position of the parameter in its ownership scope. One feature can own parameters from more than one design object, so the sort interleaves records of different objects by file position alone. We must know the field to order the list of one feature.

**Note.** The parameters that no feature owns keep a document-wide rank from a separate enumeration that counts the feature-owned parameters, so their sequence has gaps. One field holds two different orders.

## 3. Standard nested `V5_CFV2`

### SN-01. `a5 03 32` header type codes

**Question.** What does each header token `05`, `09`, `0d`, and `1d` select?

**Known.** `catia.md` §6.2 "Frames an explicit rolling-ball surface jet." admits these four width-1 tokens for an `a5 03 32` rolling-ball jet. All four use the defined degree-5 jet grammar.

**Need.** We must know the selection rule to write the correct token.

### SN-02. `a5 03 32` numeric continuation

**Question.** What fields follow the three aligned jet blocks in each numeric-continuation length class?

**Known.** `catia.md` §6.2 "Frames an explicit rolling-ball surface jet." defines the knots and the value, first-derivative, and second-derivative blocks. The continuation has more than one length class.

**Need.** We must know its lanes and terminal fields to read and write the complete record.

**Note.** `a5a8::records` accepts a continuation of up to 4096 bytes and rejects a longer one. The bound is not in `catia.md`. A record in a longer length class is dropped, so its exact rolling-ball surface and both limit curves are not transferred. The `a8` parser instead requires the 59-byte tail that the specification states.

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

### SN-10. Logical vertex to `05 08 01` allocation

**Question.** Which byte relation assigns each logical vertex component to a `05 08 01` allocation row?

**Known.** `catia.md` §5.4 "Standard `u16be` edge rows are handle sequences" defines the logical-corner quotient and physical endpoint ports independently of coordinate-row allocation.

**Need.** We must know the allocation relation for byte-faithful writing.

**Conflict.** `catia.md` §5.4.1 states an answer for one body class: "Regular-motif bodies serialize vertex allocation as a walk over the ordered trim packets", with four fixed column permutations. This item states that the relation is unknown. One of the two documents is wrong. The specification gives no byte-level derivation for the four permutations, and the two validity tests it states — the first-occurrence population equals the vertex-table count, and every circle row with exactly two on-circle vertex rows maps to that pair — are the two output tests that `missing_edge::motif_port_points` and `standard::fbb::parse_standard_motif` apply. A permuted emission order satisfies both tests. On a body with no circle rows the second test is vacuous, because `circle_anchors` is `None` for a `Line` or `Bspline` row and the caller accepts `None`.

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

### SN-30. Trim-record acceptance bounds

**Question.** What bounds the handle count of a trim record, and what tolerance applies to the norm of its `3×f32le` frame vector?

**Known.** `catia.md` §5.3 "Invariant `N == 3*A + sum(K)`" gives `ff <N:u32le>` with the invariant `N == 3*A + sum(K)`, and gives the frame vector as a unit vector. It gives no upper bound for `N` and no tolerance. `fbb::parse_trim_record_layout` rejects a record when `N` is more than 500000, or when `|norm² − 1|` is not less than `2e-4`. The `2e-4` value is about three orders of magnitude larger than the round-trip error of a `f32` unit vector.

**Need.** A rejected record leaves the predecessor set of `fbb::parse_trim_chain`. That function then can find one chain of the required length that does not hold the rejected record, and it accepts that chain as unique. We must know the true bounds to keep a valid record in the search.

### SN-42. Consolidated record census by marker

**Question.** Where does the consolidated A/B record cluster start, so that a frame walk can enumerate it?

**Known.** `catia.md` §6 "Header width and flag are independent" gives "The frame is length-closed (walking lands exactly on each next record and on the cluster end)", and gives "A literal marker scan ... is both lossy ... and noisy (in-payload coincidences); census by the frame walk, not by marker hits."

**Need.** `wire::records::consolidated_records` is the only record source for the consolidated, `a5a8`, `b2`, freeform, and standard paths, and its complement defines where `05 08 01` vertex rows are read. It is a marker scan: it accepts an offset when the lead byte is in `a5..a7` or `b2..b4`, the next byte is `03`, `13`, or `83`, and the declared length stays in bounds. It applies no length-closure test. We must know the cluster start to walk the frames as the specification requires.

**Note.** `wire::records::consolidated_records` advances to the end of an accepted frame, so marker-like bytes in that frame's header or payload do not open phantom records. It still searches for the first valid candidate in each gap and has no cluster-boundary or complete-run proof. A valid false candidate in a gap can still start a record walk before the intended cluster.

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

### OS-16. Vertex coordinate from parameter incidences

**Question.** Which class-`06` parameter incidence gives the coordinate of a logical vertex, and which domain bounds its parameter?

**Known.** `catia.md` §6.7 "**Object-stream topology:**" gives "References to typed pcurves evaluate at their paired parameter and lift through the pcurve support to the vertex locus", and gives every class-`05` roster entry as an incidence at the same vertex. `graph::incidence_vertex_coordinates` keeps the first pair that gives a finite lift and does not evaluate the others. It applies no parameter domain bound, and `bspline_span` extrapolates outside the knot domain instead of refusing.

**Need.** The result replaces the coordinate that the `05 08 01` rows give, and it bypasses the magnitude test of the row path. The sibling `graph::edge_pcurve_parameter_values` requires every incidence to agree, and `graph::pcurve_endpoints` bounds the parameter to the pcurve domain. We must know the incidence and the domain rule to keep the correct coordinate.

### OS-17. Lifted endpoint order against the `5d` vertices

**Question.** Which field gives the order of an edge's two lifted endpoints against its two `5d` vertices?

**Known.** `catia.md` §6.7 "**Object-stream topology:**" gives "Both interval endpoints evaluate to the edge's ordered `5d` vertex loci". `graph::bind_native_vertices` keeps that order when neither vertex has a coordinate. When one vertex has a coordinate it replaces the order with the shorter of the two endpoint distances. `graph::propagate_vertex_points` and `graph::propagate_vertex_component` make the same replacement. No site refuses a tie, and no site bounds the accepted distance.

**Need.** The two vertices of an edge take swapped coordinates when the order is wrong. We must know the field to bind the coordinate without a distance test.

**Note.** `b5::transfer::pcurves` makes the same decision and refuses both an exact tie and a minimum above the point tolerance. The `score` function of `graph::propagate_vertex_points` gives the same value for both orientations of a one-constraint component, so its tie-break always keeps the serialized order there, which is the case the distance test was added for.

### OS-18. Endpoint-to-row match radius

**Question.** What radius binds a lifted endpoint to an `05 08 01` coordinate row?

**Known.** `catia.md` §6.7 "**Object-stream topology:**" gives "Coincident `05 08 01` rows share an endpoint locus", and gives the lowest serialized matching row for a subset whose allocation identity is otherwise unresolved. `graph::canonical_point` accepts every row inside a ball of `1.5e-3` mm and keeps the lowest row index. `b5::transfer` uses the same `1.5e-3` value as an acceptance gate for oriented line, circle, and NURBS plans, and as the floor of every logical vertex tolerance. `catia.md` §12 gives `1e-3` mm as the on-carrier incidence tolerance, and §5.4 and §6.3 give `2e-3` mm. No section gives `1.5e-3`, and §12 gives full `f64` storage for this family.

**Need.** Two distinct vertices closer than the radius collapse into one. The lowest-row rule that the specification gives is scoped to coincident rows, not to a ball. We must know the radius, or the identity relation that removes the radius.

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

### ZE-05. Radial-twin selection without the midpoint

**Question.** Which condition lets a radial-twin candidate omit the midpoint witness?

**Known.** `catia.md` §8 "Packed loop sense orients each lifted endpoint pair" gives "Two occurrences are radial-twin candidates when their unordered endpoint pairs and midpoint points are uniquely equal within the same tolerance." Each pcurve support retains the surface point at the midpoint of its bounded parameter interval, and the decoder stores it.

**Need.** Face-incidence components partition a coincident bounded-witness group, and a component of exactly two occurrences becomes one physical edge. A wrong component merges two occurrences into one edge with one curve. We must know the condition to keep or to remove the witness.

**Conflict.** `zero_entity::topology::selected_radial_matches` returns the match without the midpoint test when the endpoint match is a mutual singleton. Two occurrences that share both endpoints on different curves, such as the two complementary arcs of a circle each used by one face, then form one physical edge. The stored midpoints would separate them. One of the two documents is wrong: the specification sentence, or the decoder.

### ZE-06. `34c8` and `345e` knot run extents

**Question.** Which field gives the length of the U knot run and of the V knot run in a `34c8` or `345e` carrier?

**Known.** `catia.md` §8 "A `34c8`/`345e` carrier stores distinct U knots" gives the field sequence and two fixed pole-grid offsets. It gives no length field and no value range. `docs/layouts/catia.toml` holds no row for either carrier. `zero_entity::records::zero_entity_nurbs_layout` reads U knots while each value is from `0.0` to `1.0` and stops at the first value equal to `1.0`. It reads V knots while each value is from `0.0` to `50.0`. It steps over an unbounded run of `10`-prefixed tokens at the V marker and again at the pole marker, and it admits an over-consumed token because its guard tests only that the extra byte count is zero or at least ten.

**Need.** The derived offsets give the pole grid and the record end, and the surface is published with `Exactness::ByteExact`. A wrong end changes the one-based global ordinal of every record after it, because a layout failure stops the record walk. A V knot vector in model units above 50 stops the run early. We must know the length fields to read the carrier without value windows.

### ZE-07. Face roster to support-run binding

**Question.** Which field binds a zero-entity face to its surface-support run?

**Known.** `catia.md` §8 "Record families:" gives "The complete face roster aligns positionally with the complete surface-support-run roster." `zero_entity::records` builds the two rosters with independent filters. A face leaves the roster when its terminal control is not `03` or `05`, which is the byte of ZE-01. A run leaves the roster when one `21xx` support in it does not parse. The code binds the two rosters when their lengths are equal.

**Need.** One drop in each roster at different positions keeps the lengths equal and moves every face between the two positions to the wrong run. We must know the field to bind a face when a roster is incomplete.

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

### E5-04. Rank-deficient plane frame

**Question.** Which field selects the in-plane frame of a rank-deficient E5 plane when the endpoint set determines only one axis line?

**Known.** `catia.md` §9 "The complete plane frame follows from its occurrence-pcurve endpoint UV values" gives the least-squares solve for a full-rank endpoint set, and gives the known plane normal as the source of the perpendicular axis for a rank-one diameter set. It then gives "Simultaneous reversal of both in-plane axes is fixed by requiring the first nonzero component of `u_axis` to be positive." That sentence is a canonicalization, not a field.

**Need.** The in-plane frame is the chart of every pcurve on the face. Simultaneous reversal keeps the normal and reflects every UV point through the origin. We must know the field to select the frame that the file used.

**Note.** This item was removed by a tree that changed no code and added the canonicalization sentence to `catia-coverage.md`. The sentence is not what the decoder does: `e5::decode::solve_e5_plane_frame` returns a lone surviving candidate whatever its sign, and applies the positivity filter only to break a tie between two surviving frames. The tie is exactly the state in which the endpoint data does not determine the frame, so the convention answers the question rather than decoding it. `catia.md` §9 states the rule without that condition, so the specification and the decoder also disagree.

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

### E5-11. Occurrence direction with a degenerate bound span

**Question.** Which field gives the parameter direction of a loop occurrence when its two bound parameters are equal?

**Known.** `catia.md` §9 "**Topology:**" gives "Their span sign relative to the pcurve's native range fixes occurrence parameter direction". A zero span has no sign. `e5::decode` then keeps the smaller of the forward and reverse endpoint errors when the two differ by more than `1e-9`. `catia.md` §12 gives about `1e-5` mm for E5 endpoint storage, so the separation test is four orders below the precision of the compared values and almost never withholds.

**Need.** The direction reverses the edge curve geometry and the coedge parameter range. We must know the field to fix the direction of a degenerate span.

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

### FV-09. Isolated face on a pole-elided freeform carrier

**Question.** Does a face whose `a8 03 34` carrier stores no inline pole grid join the transferred B-rep?

**Known.** `catia.md` §6.7 "**Object-stream topology:**" fixes loop membership. `catia.md` §6.6 "The elided-pole form places the fixed 141-byte" gives the external pole allocation, and `a5a8::records::a8_surface_from_external_grid` binds it when its byte length is unique in the stream. A carrier that keeps no grid stays an identity-bearing surface node, so a class-`21` pcurve on it lifts to no 3D endpoint, no `5e` edge on it takes a vertex locus, its `62` loop fails the endpoint conjunct, and the owning `5f` face leaves the connected graph.

**Need.** We must know whether the external allocation transfers the face, and we must hold the exclusion when it does not. Transferring the face without a grid needs invented vertex coordinates.

**Note.** This item was removed by the tree that made the elided-tail test stricter. A stricter test admits fewer carriers, so it cannot have transferred a face that was excluded before. No test and no decode-report count shows the face reaching the neutral model, and the tree's commit message states no rationale. The item is restored until one of the two exists. OS-21 holds the binding rule that the transfer depends on.

### FV-10. Float-packed fixture validity

**Question.** What object-stream and analytic-carrier records must a synthesized float-packed inner-no-FBB input contain to be a valid specimen of the variant?

**Known.** `catia.md` §6.7 "For `b5 03 29`, the 185-byte payload is" fixes the cone chart and §6.7 "`b5 03 5d` (vertex identity)" fixes the native vertex-identity chain. The committed golden fixtures for this variant satisfy neither and reach no geometry path, so no golden fixture exercises the object-stream transfer route. Route coverage is a programmatically synthesized object stream held in the crate's tests, which decodes end to end through the container.

**Need.** We must know the minimum valid record set to synthesize golden fixtures that hold the object-stream route under snapshot.

**Note.** Loop membership no longer blocks this item. `catia.md` §6.7 "**Object-stream topology:**" fixes the rule a fixture must satisfy: each `62` node is named by exactly one `5f` face, its trailing reference is that face's carrier, and its `n_refs` equals `2*edge_count+1`. The remaining blocker is the `5d` identity chain, which needs a `5d`, a class-`05` roster, class-`06` parameter incidences, and `05 08 01` vertex rows that agree with the lifted pcurve endpoints of every incident edge. A fixture that omits the chain reaches the excluded state for a carrier without an endpoint source and proves nothing about the route.

## 8. Appearance

### AP-01. `FeatureForColor` face selection

**Question.** How does `SelectingFeatureForColorUuid` select the face targeted by an `EC 03 R G B A` override that occurs with an `EB 01 R G B` all-face color?

**Known.** `catia.md` defines both color packets. The `EB` value applies to the complete face population. The `EC` value supplies the override color asset. The positional FBB rows independently store the effective face colors, so neutral face appearance binding does not depend on this UUID incidence. The application object graph contains `FeatureForColor` and `SelectingFeatureForColorUuid`, but the UUID-to-standard-face incidence is not assigned.

**Need.** We must know the incidence to preserve or write the native selection relation independently of the effective FBB presentation population.

### AP-02. Positional colour population scope

**Question.** Which condition proves that the face population of the document is the FBB face-row population?

**Known.** `catia.md` §7.4 "An inline `EB 01 R G B` value is an opaque display color" gives the positional rule and scopes it: "The FBB sequence then binds the effective colors to the standard face population." `appearance::transfer` runs after every route, and it binds the FBB colour sequence to the faces of the document by position. Its gates are the equal count of colours and faces, and the equal sequence or multiset of colours against the packet population. No gate tests which route made the faces.

**Need.** `container::identify_variant` tests `coherent_e5` before it tests the FBB run count, so a file with both a coherent E5 stream and an FBB colour run decodes its faces in E5 order and keeps the FBB rows. The colour then binds to the face at the FBB row index. We must know the condition to hold the rule inside its stated scope.
