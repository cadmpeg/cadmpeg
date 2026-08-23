# CATIA V5 `.CATPart`: Open Items

This document lists the parts of the CATIA V5 `.CATPart` format that we do not know. The specification `catia.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Container and roster

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

**Known.** `catia.md` §7.3 "The fixed bytes in the inline production are structural" defines the complete inline production. An exact paired entity-table boundary and equal entity/object cardinality admit other nonempty childless bodies as opaque bytes without reference-role assignment.

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

**Question.** Which production defines document design configurations, and which field selects the active state?

**Known.** `catia.md` §7.3 "A self-defining schema-configuration record is" and `catia.md` §7.3 "A schema-configuration-row link is" defines schema-local `Configuration` records, `configrow` successor chains, selected value schemas, and the source-ordered open intervals between rows. These productions do not define document design configurations or assign active state. `Configuration`, `configrow`, and `DesignTable` catalog entries can remain unselected by every object record and value record; catalog vocabulary alone does not establish an instance production.

**Need.** We must identify the document design-configuration production before we can transfer configuration identities or active state.

### DI-14. Schema-configuration row semantics

**Question.** What does each entity in an open `configrow`-to-successor interval represent?

**Known.** Complete successor chains fix row order. The decoder retains each intervening entity in source order.

**Need.** We must know these schema-local roles. They do not assign document configuration names, parameter overrides, body membership, or feature replay order.

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

### DI-18. Range semantic owner

**Question.** Which sketch constraint, feature/model-tolerance object, or product-manufacturing-information object owns each complete schema-selected `Range` interval?

**Known.** `catia.md` §7.3 "A catalog-resolved value selection named `Range`" defines the complete interval independently of selector cardinality and suffix framing and retains every incoming payload reference and storage selector. Range intervals occur with no slots or with finite and unset lower and upper tolerance-deviation slots. They occur in `Range`-only, `Range`/`CstAttr_Dimension`, and larger selector sequences. Exact `D8`/`81 93`, `D8`/`81 DB`, and `DC`/`81 DB` scalar suffix dialects supply the nominal independently of selector cardinality. A `Range`-only interval can carry both deviations and its nominal. Range intervals can occupy a stable `MechanicalPart` aggregate slot selected by a `_SpecList`/`FeatureFSUR` relation whose other operand carries `UserPattern` and `SimpleLimit`; sketch-versus-PMI is not an exhaustive ownership choice. Their paired and incoming classes also include presentation, TPS capture-link, limiting-element, Boolean, geometry, and catalog-manager classes. Some intervals have no incoming incidence, one incidence, duplicate references from one object, or references from multiple objects. The narrower two-selector constraint-range production retains four exact selector/code/trailer tuples. `ListAggregator` references can include unrelated and repeated identities. A shared structural design-object owner can contain feature, geometry, and presentation classes. These incidences do not distinguish sketch, feature/model-tolerance, and product-manufacturing-information ownership. A two-selector range transfers as one opaque sketch constraint only when exactly one total incoming incidence resolves to its same-graph paired source entity and object record, and that source object's complete owner chain reaches one transferred `Sketch` before another transferred feature. The source object record is retained as one unresolved native operand. If the paired interval has a finite nominal whose bits equal the finite `CstAttr_Dimension` evaluation bits, that structurally proven sketch-owned constraint also carries one neutral millimetre length parameter with the nominal framing, bits, and opcode offset retained as properties; no dimensional subtype, target, or driving role is assigned. An exact two-selector `Range`/`CstAttr_Dimension` production with a finite bit-agreeing nominal and evaluation now transfers as a targetless PMI dimension with nullable millimetre deviations when it does not enter the sketch-constraint lane. Its source entity identity remains in the PMI identity and the complete source production remains native. This transfer is separate from the sketch-owner proof; a range that enters the sketch-constraint lane is not duplicated, and `Range`-only or other selector sequences remain unresolved.

**Need.** We must identify the owning incidence and semantic subtype and target for every `Range`-only and larger selector production. The transferred two-selector dimension remains targetless until its target grammar is established.

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

**Known.** Constraint-range values and structural incidences are retained separately. An exact source-closed relation from an admitted `2DPoint` field to a complete `ConstraintDYS` field is retained as one unresolved native sketch constraint when both fields are direct members of the same transferred `Sketch` owner list and all graph, entity, design-object, class-entry, and owner identities agree. The point entity, target field and entity, target ordinal, source/reference offsets, and every outbound target-field reference occurrence with its source container, identity, offset, resolution state, and resolved target class are retained. A structurally proven sketch-owned `Range`/`CstAttr_Dimension` constraint with a finite bit-agreeing nominal carries that nominal as a neutral millimetre length parameter. No constraint type, coordinate, dimensional subtype, driving or active state, or solver semantics is assigned.

**Need.** We must know the fields to transfer and solve the constraints.

### DI-23. Feature-instance grammar

**Question.** How do operation fields, definition-bound values, and structurally owned operand objects form one feature instance?

**Known.** `catia.md` §7.3 "All `7C09` records in one graph carrying the same `owner_ref`" defines each incidence independently. Paired entity tables admit opaque childless object records and preserve their complete bodies, but do not assign roles inside those bodies. A complete two-definition value chain with a supported second role transfers one typed parameter, but it does not assign that parameter to an operation role. Operation-named field records can share one class identity across several structural owner groups. A `Hole` class cohort contains one empty self-classified record plus list, atom-vector, mixed, or empty records under other owners. The shared class identity does not establish whether the cohort is a schema object, one feature instance, or a field program reused by multiple instances. An operation-named field class or field vocabulary does not assign feature identity, operands, outputs, or replay order. An exact separator-form owner declaration for `GSMPlaneAngle` or `GSMPlaneOffset`, with matching class entry, owner entity, and structural owner, establishes an unresolved constructed-reference-plane family node; support, angle, signed offset, normal, in-plane frame, and construction dependencies remain unresolved. An exact separator-form owner declaration for an admitted operation class, with matching class entry, owner entity, and structural owner, establishes the corresponding unresolved family node: `Prism_EndLimit_Length`, `Prism_ThickThin1`, and `Prism_ThickThin2` are unresolved extrusions, `Revol_ThickThin1` is an unresolved revolution, `Sweep_ThickThin1` is a sweep with unresolved section, path, and result mode, `EdgeFillet` is an unresolved fillet, and `CircPattern_RadialNumber` is an unresolved circular pattern. Definition-bound values remain source properties and feature-owned expressions remain typed model parameters plus source-property copies. These family and value assignments do not assign profiles, directions, axes, extents, outputs, edge groups, radii, pattern seeds, pattern axis, pattern angle, pattern count, operation-specific dependency roles, or replay order. An exact payload reference from a transferred feature object's field to a different transferred feature, or to a target whose complete owner-design-object chain reaches that feature, with an earlier feature ordinal transfers one deduplicated structural dependency in relation order. Storage selectors, unresolved targets, incomplete or cyclic owner chains, self-links, and forward targets do not. This structural edge does not identify an operation-specific input role.

**Need.** We must know the operation-specific binding that transfers profiles, directions, extents, outputs, and dependency roles for each admitted feature family, including regeneration semantics.

### DI-27. Feature-local parameter order

**Question.** Which field gives the position of a parameter in the parameter list of its feature?

**Known.** `catia.md` §7.3 "An object graph is preceded by" gives the byte offset of a design object's first field and the zero-based position of that object in the graph. It gives no order for the parameters of one feature. `crates/cadmpeg-codec-catia/src/design_feature.rs:256-286` sorts the exact feature-owned parameters by object-record byte offset, then by entity byte offset, then by the identifier that the codec makes, and publishes that rank in the field that `crates/cadmpeg-ir/src/features.rs:289` documents as the position among parameters in the same ownership scope. One feature can own parameters from more than one design object, so the sort interleaves records of different objects by file position alone.

**Need.** We must know the field to order the parameter list of one feature.

### DI-28. Saved-view and annotation ownership

**Question.** Which source relation assigns saved-view configurations and PMI annotation records to their active configuration and typed owner?

**Known.** `catia.md` §7.3 defines schema-local configuration records and row links, but does not define document design configurations, active saved-view state, or the owner relation for dimensional and feature-control annotations. The neutral model separates configurations and PMI from native design-object records.

**Need.** We must identify the configuration/view and annotation owner relations before transferring saved-view state, note ownership, datum ownership, or typed PMI.

### DI-29. Exact alias lead `0x00000133`

**Question.** Which storage role does the fixed outer-alias lead value `0x00000133` select?

**Known.** The lead precedes a complete fixed alias core. Its F1 ordinal links the alias tag to one object-graph record and, when present, that record's owner design object. The ordinal identifies design-history storage and does not identify a consolidated freeform carrier.

**Need.** We must identify the lead's storage role before assigning semantics beyond the retained alias and object-graph relations.

## 3. Standard nested `V5_CFV2`

### SN-01. `a5 03 32` header type codes

**Question.** What does each header token `05`, `09`, `0d`, and `1d` select?

**Known.** `catia.md` §6.2 "Frames an explicit rolling-ball surface jet." admits these four width-1 tokens for an `a5 03 32` rolling-ball jet. All four use the defined degree-5 jet grammar.

**Need.** We must know the selection rule to write the correct token.

### SN-02. `a5 03 32` numeric continuation

**Question.** What fields follow the three aligned jet blocks in each numeric-continuation length class?

**Known.** `catia.md` §6.2 "Frames an explicit rolling-ball surface jet." defines the knots and the value, first-derivative, and second-derivative blocks. The continuation has more than one length class.

**Need.** We must know its lanes and terminal fields to read and write the complete record.

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

**Known.** `catia.md` §6.3 "A complete consolidated edge run is" defines exact two-surface constructions from class-`0x20` pcurve jets, their supports, and their shared parameter interval. A sphere-plane standard spline with a secant plane and endpoints on the exact section is a full-circle carrier, an oblique cylinder-plane standard spline is a full-ellipse carrier, and a perpendicular-cylinder standard spline with radii equal within tolerance is the full ellipse branch selected by its endpoints; none has an edge interval or pcurve unless a branch witness or native parameter incidence exists. The separate 3D cache remains an opaque carrier.

**Need.** We must know the cache program to read a spline when the exact construction is unavailable and to write the cache.

### SN-12. Consolidated persistent-tag namespace

**Question.** How does an `op1` or absolute persistent CGM tag select a serialized record outside the exact class-`0x19` circle and embedded type-`3` cylinder bindings?

**Known.** `catia.md` §6.3 "When the `op1` support identity equals" defines the exact unique class-`0x19` binding and the unique embedded type-`3` cylinder binding. The decoder retains other persistent identities.

**Need.** We must resolve the namespace to bind other consolidated curve and support records.

### SN-14. Multiple FBB face groups

**Question.** Which fields assign standard-path faces and edges to topology components when the file has multiple separate FBB face groups?

**Known.** `catia.md` §5.1 "trim_record[i] -> face_outer_bound_row[i] -> face i" defines topology reconstruction for one positional spine and its FBB incidences.

**Need.** We must know cross-group membership to build all bodies and shells.

**Conflict.** `selected_standard_run` in
`src/families/standard/fbb.rs` falls back to `largest_fbb_run` when no
source-closed FBB group is reconstructed. The fallback selects a unique largest
marker run by row count alone. Neither row count nor uniqueness by size binds
that run as the governing topology spine, so a larger secondary or unrelated
population can displace the required face population.

### SN-15. Standard arc branch selection

**Question.** Which serialized field or identity relation selects the standard arc branch?

**Known.** `catia.md` §5.6 "**Circle/arc endpoints by support intersection:**" defines arc selection when one of these witnesses fixes the branch. `catia.md` §5.7 defines a centered sphere as no circle-plane witness; a distinct incident carrier, such as a cylinder, can still define the full circle, but it does not select an arc branch.

**Need.** We must define branch selection for records in which those relations do not select one branch.

### SN-16. Class-`0x20` persistent reference

**Question.** How does the `op1` or persistent-tag reference in an `a5 03 20` record select a serialized record?

**Known.** `catia.md` §6.3 "`b2 03 20` is the B-family form" defines the pcurve payload and support binding for exact identity and chart matches.

**Need.** We must resolve other references to bind the pcurve support.

### SN-17. Class-`0x62` owner bounds

**Question.** What do the binary64 and binary32 lanes represent in tagged alternating and width-coded fixed-nine owner packets?

**Known.** In the all-compact grammar, the three binary32 pairs are the model-space X, Y, and Z bounds of the owned face boundary. In tagged alternating packets with an established NURBS-carrier relation, the binary64 pairs are the carrier parameter rectangle and may cover a proper subdomain. The tagged binary32 lane is not a direct face-boundary box. Width-coded binary32 bounds can enclose more than one face boundary.

**Need.** We must define the binary64 lane in the all-compact and width-coded grammars and the binary32 lane in the tagged alternating and width-coded grammars.

### SN-18. Class-`0x62` owner-to-face binding

**Question.** Which field binds a fixed `b2`, `b3`, or `b4 03 62` owner packet to its face record?

**Known.** A complete class-`0x5f` node immediately preceding a fixed-nine owner binds to that owner when its target's checked successor is the ninth owner identity. Terminal `03 05` admits every fixed-nine grammar. Terminal `03 03` admits the all-compact grammar only. The node target and owner identities are allocation-local.

**Need.** We must identify the allocation-group relation that maps the bound node target to a standard face ordinal without comparing local identities across groups.

**Note.** The all-compact model-space box can nominate one geometric face, but this geometric witness does not define the source identity join.

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

**Question.** What higher-level object does each derived `b2`, `b3`, or `b4 03 5f` to `0x62` packet relation represent?

**Known.** A structurally complete class-`0x5f` face node retains either its compact or tagged-`u16` target encoding and its two terminal bytes. The `03 05` terminal form retains a derived relation to an immediately adjacent class-`0x62` packet when the checked successor identity matches the packet's final reference. The `03 03` terminal form retains the same relation for an all-compact fixed-nine packet. Other terminal and packet-grammar combinations remain unassigned. The relation does not assign a higher-level object or allocation role.

**Need.** We must know the object role to assign the owner to a feature or face.

### SN-26. Revolution profile allocation identity

**Question.** How does the `u16le` profile allocation identity in a `b2 03 2d` revolution record select its native profile record?

**Known.** `catia.md` §5.15 "The `00 33 30` byte is only" defines the axis frame, angular chart, and exact unique profile-interval binding.

**Need.** We must define the allocation namespace and directrix relation when profile intervals do not identify one record.

### SN-27. Consolidated class-`0x27` plane carrier

**Question.** What semantic roles do the four tail scalars of an `ec` class-`0x27` record have, and what supplies its missing plane axis frame?

**Known.** A complete `b2`, `b3`, or `b4 03 27` frame has payload marker `b4`, a selector, and a nonempty finite f64 lane. Selector `e4` carries seven values as a two-coordinate point, a two-coordinate direction with an omitted third component, and a three-scalar tail. Selector `c4` carries eight values as a two-coordinate point, a three-coordinate direction, and a three-scalar tail. Selector `ec` carries six values as a two-coordinate point and a four-scalar tail. Other selectors retain their complete finite scalar lanes without a neutral plane layout. The direction-bearing `e4` and `c4` layouts define a plane frame with origin `(point_x,point_y,0)`, the stored unit direction as `u_axis`, global Z as the second chart axis, and `unit(u_axis×Z)` as the normal. Their tails are finite with a positive first scalar and an increasing final pair. A direction-bearing carrier binds a consolidated pcurve side only when endpoint lifts select exactly one carrier and reach the object-stream vertices. A directionless `ec` carrier is retained in the native namespace and does not supply a neutral plane support.

**Need.** Resolve the `ec` tail roles and its axis-frame source before transferring that layout as a neutral plane support.

### SN-28. Standard limit-curve point occurrence binding

**Question.** Which parameter occurrence on a standard degree-5 limit curve names a serialized endpoint when more than one parameter is within the point tolerance?

**Known.** `catia.md` §5.8 "A standard spline edge with two distinct adjacent face carriers" requires every within-tolerance candidate for one endpoint to occupy one parameter-tolerance cluster. The decoder rejects any separated occurrence, independent of residual ordering.

**Need.** We must know the endpoint-occurrence identity to bind a curve that has separated candidates instead of retaining it natively.

### SN-33. Spine grammar arbitration

**Question.** Which container field distinguishes a standard-nested spine from an FBB-only spine when the file admits both edge-table grammars, or neither?

**Known.** `catia.md` §1 "Detection invariants: a standard file has one nested inner" gives the detection invariants, and the variant table gives an FBB-only spine as a nested container with FBB face rows and `05 08 01` vertices but no standard edge-row table. An admitted standard edge-table grammar establishes a complete nested FBB spine and owns route selection over a coherent E5 walk. When only the FBB-only grammar is admitted, the spine is partial and a coherent E5 walk owns route selection; without that walk, the FBB-only route applies. `crates/cadmpeg-codec-catia/src/container.rs` tries the standard edge-table grammar first, then the FBB-only two-table grammar, and then classifies by the count of `EDGE_DELIMITER` occurrences. The delimiter is not sufficient to distinguish the two grammars because FBB-only widths one and three reuse it.

**Need.** We must resolve the remaining state where neither edge-table grammar admits, and establish the source rule if both grammars admit the same byte region. The variant fixes the decode route: `StandardNested` uses the complete standard spine, `FbbOnly` uses the partial-spine route, and `E5Stream` uses the coherent E5 graph.

### SN-39. Compact edge explicit endpoint selectors

**Question.** Which identity namespace do the `4n+2`, `06 <u8>`, and `0a <u16le>` endpoint selector forms in a width-coded class-`0x5e` edge node address?

**Known.** `catia.md` §6 defines the explicit forms as distinct from the local child and backward allocation walk. Paired `4w` middle references are forward distances in the complete consolidated framed-record sequence; both must land on B-family class-`0x18` endpoint records before their target-record offsets become ordered vertex identities. The decoder retains the other forms and their numeric operands without joining them to an endpoint record. A complete adjacent edge-use run types the middle pair as endpoints and the final pair as side selectors. A standalone five-reference record does not establish those roles. Explicit-reference populations contain complete repeated-value relations in both pairs, so numeric repetition alone does not select the endpoint pair.

**Need.** We must define the target namespace and scope of each remaining explicit form, including whether different forms can name one endpoint record.

### DI-24. PMI dimension quantity and suffix framing

**Question.** Which field gives the physical quantity of a transferred `Range`/`CstAttr_Dimension` nominal and its deviations, and what do the `B8`, `C1`, and `DC` suffix framings select?

**Known.** `DI-18` records that the semantic subtype of a `Range` production is unknown. `crates/cadmpeg-codec-catia/src/pmi.rs:142-148` gives the length quantity to the nominal and to both deviations of every admitted production. `crates/cadmpeg-ir/src/pmi.rs` defines that quantity as millimetres and defines an angle quantity and a ratio quantity beside it. `crates/cadmpeg-codec-catia/src/pmi.rs:79-88` admits three exact suffix framings. `crates/cadmpeg-codec-catia/src/pmi.rs:101-107` gives all three the same dimension kind, and the neutral annotation record keeps no native reference, so the framing does not reach the neutral model. The transfer emits a targetless PMI dimension with millimetre nominal and deviations when the exact production is not proven sketch-owned; a proven sketch-owned production instead carries one neutral millimetre length parameter. `crates/cadmpeg-codec-catia/src/loss.rs` has no code for this unresolved semantic distinction.

**Need.** We must know the quantity to transfer the value. An angular dimension in radians and a length in millimetres are not distinguishable in the current output.

### DI-25. Compact `1A` operation-class declaration

**Question.** Does a compact self-owned `1A` root declare an operation class?

**Known.** `catia.md` §7.3 "An exact separator-form owner declaration with class name `GSMPlaneAngle`" requires a separator-form declaration with a matching class entry, owner entity, and structural owner for each unresolved operation family node. `crates/cadmpeg-codec-catia/src/design_feature.rs:713-741` also admits a self-owned compact `1A` root that has no structural owner, and reads the operation class name and class entry from that record. Commits on this branch added classes to the admitted operation set, so the compact route now reaches the reference-plane, extrusion, and circular-pattern families.

**Need.** We must know whether the compact root declares a class, because the family node it makes is a neutral feature with a history ordinal.

**Conflict.** `catia.md` §7.3 "All `7C09` records in one graph carrying the same `owner_ref`" states that in compact groups the selected record is an identity anchor and not a class declaration, and that owner class and storage stay unset. `crates/cadmpeg-codec-catia/src/design_feature.rs:728-740` reads the owner class name and class entry from that record.

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

## 5. Zero-entity `a9 03`

### ZE-01. Class-`0x5fxx` face terminal control

**Question.** What does the terminal control byte in each `0x5fxx` face record control?

**Known.** `catia.md` §8 "Record framing `a9 03 XX YY <payload[YY+8]>`, `record_length = YY + 12`" defines the zero-entity face roster and support incidences.

**Need.** We must know the control to validate and write the face.

### ZE-02. Oriented-use allocation lane

**Question.** Which allocation-lane rule associates a `0638` oriented use with its owner-local `0x21xx` support and `0x05xx` incidence record?

**Known.** `catia.md` §8 "Record families:" defines these record populations and their owner-local identities independently.

The `5e1a` tuple does not provide this missing join: its `T`, `T−1`, and `T−2` values belong to the `0638`/`2569` topology namespace, while `X` and `Y` select the two adjacent surface-support slots.

**Need.** We must know the association to build neutral coedges.

### ZE-03. Physical-edge endpoint binding

**Question.** Which fields bind a `05 0b`, `05 10`, or `05 15` incidence allocation lane to its physical-edge endpoints?

**Known.** The decoder retains the incidence lanes and endpoint coordinate rows.

**Need.** We must know the binding to build neutral edges and vertices.

### ZE-04. `5e 1a` allocation namespaces

**Question.** What does each independent `T`, `X`, and `Y` allocation in the `5e 1a` tuple `[T,X,Y,T−1,T−2]` select?

**Known.** `catia.md` §8 "A `5e1a` edge-stride record contains" defines the tuple relation and retains all five identities.

**Need.** We must know the namespaces to bind the edge, supports, and incidences.

### ZE-05. Zero-entity ownership-root selection

**Question.** How does a zero-entity stream select its ownership root when more than one face-roster/shell/body triple is structurally valid?

**Known.** `zero_entity_ownership_roots` recognizes every contiguous `[0x6142, 0x6006, 0x6508]` triple with checked fields and retains the candidates in source order. The neutral zero-entity route binds ownership only when exactly one candidate exists. Multiple candidates remain native and do not select a body or shell.

**Need.** We must know the terminal/root identity rule or reject multiple valid ownership triples before transferring body and shell ownership.

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

### E5-11. NURBS carrier trailing lanes

**Question.** What fields do the fixed trailing lanes of class-`0xaa` p-curves and class-`0xe7` surface carriers encode?

**Known.** `catia.md` §9 defines the complete NURBS knot, multiplicity, control-point, degree, mode, and weight productions. Class `0xaa` retains a fixed 37-byte trailing lane. Class `0xe7` retains a fixed 148-byte trailing lane. The lanes are required for record admission but are not assigned to topology, geometry, or parameter semantics.

**Need.** We must identify the fields in both lanes before decoding or writing their internal values.

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

### FV-09. External pole-grid ownership

**Question.** Which identity relation binds an elided-pole `a8 03 34` carrier to its external pole grid?

**Known.** `catia.md` §6.6 defines the elided-pole carrier and external pole allocation. Object-stream topology independently defines pcurve, edge, loop, and face ownership.

**Need.** We must define the grid identity and scope that bind the carrier, its pcurves, and its topology to one external pole allocation.

### FV-10. Float-packed fixture validity

**Question.** What object-stream and analytic-carrier records must a synthesized float-packed inner-no-FBB input contain to be a valid specimen of the variant?

**Known.** `catia.md` §6.7 "For `b5 03 29`, the 185-byte payload is" fixes the cone chart and §6.7 "`b5 03 5d` (vertex identity)" fixes the native vertex-identity chain. The committed freeform fixtures include geometry-transferred A5 and A8 carriers, but they do not exercise the object-stream transfer route: that route has no face, loop, edge, coedge, or vertex population in those fixtures. Route coverage is a programmatically synthesized object stream held in the crate's tests, which decodes end to end through the container.

**Need.** We must know the minimum valid record set to synthesize golden fixtures that hold the object-stream route under snapshot.

## 8. Appearance

### AP-01. `FeatureForColor` face selection

**Question.** How does `SelectingFeatureForColorUuid` select the face targeted by an `EC 03 R G B A` override that occurs with an `EB 01 R G B` all-face color?

**Known.** `catia.md` defines both color packets. The `EB` value applies to the complete face population. The `EC` value supplies the override color asset. The positional FBB rows independently store the effective face colors, so neutral face appearance binding does not depend on this UUID incidence. The application object graph contains `FeatureForColor` and `SelectingFeatureForColorUuid`, but the UUID-to-standard-face incidence is not assigned.

**Need.** We must know the incidence to preserve or write the native selection relation independently of the effective FBB presentation population.
