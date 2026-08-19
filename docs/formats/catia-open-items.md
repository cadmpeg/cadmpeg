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

**Note.** Reopened by the 2026-08-18 closure audit; this is the second reopening. The 2026-08-10 audit reopened the item because commit `5f1d8cb2d` changed only this ledger. Commit `54a9ccb3a` closes it again and changes only `catia.md` and this ledger. It adds the framing-specific slot and `paramout` class rule to `catia.md` §7.3 "In both productions, `self` equals the paired entity identity." The rule it adds is the rule `native.rs` already applied, so the change writes decoder policy into the specification and gives it format authority. The commit body names synthetic lead-`12`, lead-`54`, and transfer tests as the cover. The earlier Note rejected that same evidence, because the tests are built from the rule they test. The specification clause carries no `CADIR decision` marker, and no cost is charged when the slot class is not `paramout`. Corpus byte records must show which slot carries the result before this item closes.

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

**Note.** Reopened by the 2026-08-10 closure audit. Commit `0d0e50f98` settles only complete same-run local input packets and explicitly leaves nonlocal or unresolved input selectors unresolved. That supported subset does not answer which packet supplies every typed `String` input. The spec change and synthetic consistency tests do not identify the native input join.

### DI-11. Evaluated `String` relation results

**Question.** Which string-value entity stores the result of a typed `String` relation?

**Known.** `catia.md` §7.1 "A typed relation consists of" and `catia.md` §7.1 "A legacy string-value packet is" defines string packets and typed relation result signatures. A self-`body` relation whose `param` selector resolves one same-run identity selects that identity. For a zero-input `VoidType` relation, the exact output-assignment rule in `catia.md` §7.1 transfers that selected string packet when its value agrees with the right-hand result. A relation with input clauses or a nonlocal selector still does not establish its result packet.

**Need.** We must know the result entity for input-bearing relations and relations with nonlocal selectors so we can transfer the evaluated value.

**Note.** Reopened by the 2026-08-10 closure audit. Commit `0d0e50f98` documents the zero-input/local subset already stated above; it does not settle the original input-bearing and nonlocal result question. Agreement on that subset does not verify the remaining result ownership. Read input-bearing and nonlocal `String` relations in corpus parts to fix that ownership.

### DI-12. Typed `Boolean` value production

**Question.** What byte production stores the scalar value of a typed `Boolean` parameter?

**Known.** `catia.md` §7.3 "A complete entity-record suffix value begins" defines scalar, unset, atom, control, and schema-selected object values. Boolean-named field classes can contain compound object payloads.

**Need.** We must know the scalar production to transfer a Boolean parameter.

**Note.** Reopened by the 2026-08-10 closure audit. Commit `d0f630db0` adds 0/1 atom handling and synthetic tests built by the same fixture helper, then states that mapping as settled. Corpus and probe-batch records have not yet verified that this atom is the Boolean scalar production; a Boolean-named field can carry other value forms. Author a probe batch that writes known Boolean parameters and compare the stored bytes.

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

**Known.** `catia.md` §7.3 "A catalog-resolved value selection named `Range`" defines the complete interval independently of selector cardinality and suffix framing and retains every incoming payload reference and storage selector. Range intervals occur with no slots or with finite and unset lower and upper tolerance-deviation slots. They occur in `Range`-only, `Range`/`CstAttr_Dimension`, and larger selector sequences. Exact `D8`/`81 93`, `D8`/`81 DB`, and `DC`/`81 DB` scalar suffix dialects supply the nominal independently of selector cardinality. A `Range`-only interval can carry both deviations and its nominal. Range intervals can occupy a stable `MechanicalPart` aggregate slot selected by a `_SpecList`/`FeatureFSUR` relation whose other operand carries `UserPattern` and `SimpleLimit`; sketch-versus-PMI is not an exhaustive ownership choice. Their paired and incoming classes also include presentation, TPS capture-link, limiting-element, Boolean, geometry, and catalog-manager classes. Some intervals have no incoming incidence, one incidence, duplicate references from one object, or references from multiple objects. The narrower two-selector constraint-range production retains four exact selector/code/trailer tuples. `ListAggregator` references can include unrelated and repeated identities. A shared structural design-object owner can contain feature, geometry, and presentation classes. These incidences do not distinguish sketch, feature/model-tolerance, and product-manufacturing-information ownership. A two-selector range transfers as one opaque sketch constraint only when exactly one total incoming incidence resolves to its same-graph paired source entity and object record, and that source object's complete owner chain reaches one transferred `Sketch` before another transferred feature. The source object record is retained as one unresolved native operand; neutral sketch entities, loci, parameters, dimensional roles, and other Range-bearing selector sequences remain unresolved. An exact two-selector `Range`/`CstAttr_Dimension` production with a finite bit-agreeing nominal and evaluation now transfers as a targetless PMI dimension with nullable millimetre deviations. Its source entity identity remains in the PMI identity and the complete source production remains native. This transfer is separate from the sketch-owner proof; a range that enters the sketch-constraint lane is not duplicated, and `Range`-only or other selector sequences remain unresolved.

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

**Known.** Constraint-range values and structural incidences are retained separately. An exact source-closed relation from an admitted `2DPoint` field to a complete `ConstraintDYS` field is retained as one unresolved native sketch constraint when both fields are direct members of the same transferred `Sketch` owner list and all graph, entity, design-object, class-entry, and owner identities agree. The point entity, target field and entity, target ordinal, source/reference offsets, and every outbound target-field reference occurrence with its source container, identity, offset, resolution state, and resolved target class are retained. No constraint type, coordinate, parameter, driving or active state, or solver semantics is assigned.

**Need.** We must know the fields to transfer and solve the constraints.

### DI-23. Feature-instance grammar

**Question.** How do operation fields, definition-bound values, and structurally owned operand objects form one feature instance?

**Known.** `catia.md` §7.3 "All `7C09` records in one graph carrying the same `owner_ref`" defines each incidence independently. Paired entity tables admit opaque childless object records and preserve their complete bodies, but do not assign roles inside those bodies. A complete two-definition value chain with a supported second role transfers one typed parameter, but it does not assign that parameter to an operation role. Operation-named field records can share one class identity across several structural owner groups. A `Hole` class cohort contains one empty self-classified record plus list, atom-vector, mixed, or empty records under other owners. The shared class identity does not establish whether the cohort is a schema object, one feature instance, or a field program reused by multiple instances. An operation-named field class or field vocabulary does not assign feature identity, operands, outputs, or replay order. An exact separator-form owner declaration for `GSMPlaneAngle` or `GSMPlaneOffset`, with matching class entry, owner entity, and structural owner, establishes an unresolved constructed-reference-plane family node; support, angle, signed offset, normal, in-plane frame, and construction dependencies remain unresolved. An exact separator-form owner declaration for an admitted operation class, with matching class entry, owner entity, and structural owner, establishes the corresponding unresolved family node: `Prism_EndLimit_Length`, `Prism_ThickThin1`, and `Prism_ThickThin2` are unresolved extrusions, `Revol_ThickThin1` is an unresolved revolution, `Sweep_ThickThin1` is a sweep with unresolved section, path, and result mode, `EdgeFillet` is an unresolved fillet, and `CircPattern_RadialNumber` is an unresolved circular pattern. Definition-bound values remain source properties and feature-owned expressions remain typed model parameters plus source-property copies. These family and value assignments do not assign profiles, directions, axes, extents, outputs, edge groups, radii, pattern seeds, pattern axis, pattern angle, pattern count, operation-specific dependency roles, or replay order. An exact payload reference from a transferred feature object's field to a different transferred feature, or to a target whose complete owner-design-object chain reaches that feature, with an earlier feature ordinal transfers one deduplicated structural dependency in relation order. Storage selectors, unresolved targets, incomplete or cyclic owner chains, self-links, and forward targets do not. This structural edge does not identify an operation-specific input role.

**Need.** We must know the operation-specific binding that transfers profiles, directions, extents, outputs, and dependency roles for each admitted feature family, including regeneration semantics.

### DI-27. Feature-local parameter order

**Question.** Which field gives the position of a parameter in the parameter list of its feature?

**Known.** `catia.md` §7.3 "An object graph is preceded by" gives the byte offset of a design object's first field and the zero-based position of that object in the graph. It gives no order for the parameters of one feature. `crates/cadmpeg-codec-catia/src/design_feature.rs:256-286` sorts the exact feature-owned parameters by object-record byte offset, then by entity byte offset, then by the identifier that the codec makes, and publishes that rank in the field that `crates/cadmpeg-ir/src/features.rs:289` documents as the position among parameters in the same ownership scope. One feature can own parameters from more than one design object, so the sort interleaves records of different objects by file position alone.

**Need.** We must know the field to order the parameter list of one feature.

**Note.** Recovered from a 2026-08-08 audit pass whose ledger commits are reachable from no ref. The identifier is the original one. The document-scope half of the original item is settled: `crates/cadmpeg-codec-catia/src/design_feature.rs:295-315` gives the parameters that no feature owns their own contiguous scope.

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

### SN-10. Logical vertex to `05 08 01` allocation

**Question.** Which byte relation assigns each logical vertex component to a `05 08 01` allocation row?

**Known.** `catia.md` §5.4 "Full-form standard `u16be` edge rows are handle sequences" defines the logical-corner quotient and physical endpoint ports independently of coordinate-row allocation. The same section defines evidence-free topology automorphisms that can jointly permute unbound edge rows and coordinate-row labels without assigning a byte-level coordinate allocation.

**Need.** We must know the allocation relation for byte-faithful writing.

**Conflict.** `catia.md` §5.4.1 states an answer for one body class: "Regular-motif bodies serialize vertex allocation as a walk over the ordered trim packets", with four fixed column permutations. This item states that the relation is unknown. One of the two documents is wrong. The specification gives no byte-level derivation for the four permutations, and the two validity tests it states — the first-occurrence population equals the vertex-table count, and every circle row with exactly two on-circle vertex rows maps to that pair — are the two output tests that `missing_edge::motif_port_points` and `standard::fbb::parse_standard_motif` apply. A permuted emission order satisfies both tests. On a body with no circle rows the second test is vacuous, because `circle_anchors` is `None` for a `Line` or `Bspline` row and the caller accepts `None`.

**Note.** Reopened by the 2026-08-10 closure audit. Commit `693c263ce` only rejects an incomplete walk and adds a synthetic test; it does not derive the four allocation permutations. The documented rule is therefore a promotion of the current solver behavior, not byte-level evidence.

### SN-11. Standard 3D spline cache

**Question.** How does the separate standard 3D spline cache encode its poles and knots?

**Known.** `catia.md` §6.3 "A complete consolidated edge run is" defines exact two-surface constructions from class-`0x20` pcurve jets, their supports, and their shared parameter interval. A sphere-plane standard spline with a secant plane and endpoints on the exact section is a full-circle carrier, an oblique cylinder-plane standard spline is a full-ellipse carrier, and a perpendicular-cylinder standard spline with radii equal within tolerance is the full ellipse branch selected by its endpoints; none has an edge interval or pcurve unless a branch witness or native parameter incidence exists. The separate 3D cache remains an opaque carrier.

**Note.** Generated analytic circles and ellipses use canonical angular edge parameters. Their intcurve contexts map those parameters to the native support-pcurve interval and reverse the support pcurves when endpoint order requires it.

**Need.** We must know the cache program to read a spline when the exact construction is unavailable and to write the cache.

### SN-12. Consolidated persistent-tag namespace

**Question.** How does an `op1` or absolute persistent CGM tag select a serialized record outside the exact class-`0x19` circle and embedded type-`3` cylinder bindings?

**Known.** `catia.md` §6.3 "When the `op1` support identity equals" defines the exact unique class-`0x19` binding and the unique embedded type-`3` cylinder binding. The decoder retains other persistent identities.

**Need.** We must resolve the namespace to bind other consolidated curve and support records.

### SN-13. Standard `0x60` local tag binding

**Question.** How does a standard `0x60` row local allocation tag bind to its native edge record when no edge node has the same curve identity?

**Known.** `catia.md` §5.5 `edge_support_row` defines exact identity binding and the endpoint-incidence fallback. An evaluated exact support pcurve corroborates the row's unordered endpoint pair. Its wrapper order does not override the direction selected by a native endpoint identity source; a distinct unordered pair remains a conflict.

**Need.** We must know the remaining binding to transfer the native edge carrier.

**Note.** The endpoint-incidence fallback does not establish the remaining `0x60` carrier binding. It only selects an endpoint identity when one unused native edge has one distinct unordered pair in the row's geometric domain. A repeated pair remains unresolved.

### SN-14. Multiple FBB face groups

**Question.** Which fields assign standard-path faces and edges to topology components when the file has multiple separate FBB face groups?

**Known.** `catia.md` §5.1 "trim_record[i] -> face_outer_bound_row[i] -> face i" defines topology reconstruction for one positional spine and its FBB incidences.

**Need.** We must know cross-group membership to build all bodies and shells.

### SN-15. Standard arc branch without a witness

**Question.** Which arc branch applies when no adjacent face witness and no exact two-support object-stream pcurve witness is available?

**Known.** `catia.md` §5.6 "**Circle/arc endpoints by support intersection:**" defines arc selection when one of these witnesses fixes the branch. `catia.md` §5.7 defines a centered sphere as no circle-plane witness; a distinct incident carrier, such as a cylinder, can still define the full circle, but it does not select an arc branch.

**Need.** We must know the remaining selector to reconstruct the arc.

### SN-16. Class-`0x20` persistent reference

**Question.** How does the `op1` or persistent-tag reference in an `a5 03 20` record select a serialized record?

**Known.** `catia.md` §6.3 "`b2 03 20` is the B-family form" defines the pcurve payload and support binding for exact identity and chart matches.

**Need.** We must resolve other references to bind the pcurve support.

**Note.** Reopened by the 2026-08-10 closure audit. Commit `ca564c382` changes only documentation and adds a separate embedded type-3 cylinder case to SN-12. It does not answer the remaining `a5 03 20` persistent-reference namespace.

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

**Note.** Reopened by the 2026-08-10 closure audit. Commit `10b187f9f` changed only documentation. The current resolver and synthetic tests cover exact record identity and a unique interval fallback, but they do not establish the allocation namespace or directrix rule for non-unique and other unresolved cases.

### SN-27. Consolidated class-`0x27` plane carrier

**Question.** What semantic roles do the four tail scalars of an `ec` class-`0x27` record have, and what supplies its missing plane axis frame?

**Known.** A complete `b2`, `b3`, or `b4 03 27` frame has payload marker `b4`, a selector, and a nonempty finite f64 lane. Selector `e4` carries seven values as a two-coordinate point, a two-coordinate direction with an omitted third component, and a three-scalar tail. Selector `c4` carries eight values as a two-coordinate point, a three-coordinate direction, and a three-scalar tail. Selector `ec` carries six values as a two-coordinate point and a four-scalar tail. Other selectors retain their complete finite scalar lanes without a neutral plane layout. The direction-bearing `e4` and `c4` layouts define a plane frame with origin `(point_x,point_y,0)`, the stored unit direction as `u_axis`, global Z as the second chart axis, and `unit(u_axis×Z)` as the normal. Their tails are finite with a positive first scalar and an increasing final pair. A direction-bearing carrier binds a consolidated pcurve side only when endpoint lifts select exactly one carrier and reach the object-stream vertices. A directionless `ec` carrier is retained in the native namespace and does not supply a neutral plane support.

**Need.** Resolve the `ec` tail roles and its axis-frame source before transferring that layout as a neutral plane support.

### SN-28. Standard limit-curve point occurrence binding

**Question.** Which parameter occurrence on a standard degree-5 limit curve names a serialized endpoint when more than one parameter is within the point tolerance?

**Known.** `catia.md` §5.8 "A standard spline edge with two distinct adjacent face carriers" requires every within-tolerance candidate for one endpoint to occupy one parameter-tolerance cluster. The decoder rejects any separated occurrence, independent of residual ordering.

**Need.** We must know the endpoint-occurrence identity to bind a curve that has separated candidates instead of retaining it natively.

### SN-32. Derived analytic carrier arc without a witness

**Question.** Which arc of a derived analytic carrier is the edge when no branch witness and no native parameter incidence is available?

**Known.** `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:7144-7159` gives a parameter interval to every derived circle and ellipse that has no interval. `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:6901-6920` uses the short arc between the two endpoints when the witness is absent. The witness needs two resolved support carriers whose lifted midpoints agree inside `1e-6`, so it is absent for a derived carrier with no native support. `crates/cadmpeg-codec-catia/src/nurbs.rs:370-374` rejects a sweep that is not positive, so the short arc is accepted for one sense of the stored plane normal and refused for the opposite sense of the same geometry. `SN-15` asks for the arc selector and stays open.

**Need.** We must know the selector, because the short arc and the long arc are both admissible and the neutral edge covers only one of them.

**Conflict.** `catia.md` §5.8 "A standard spline edge with two distinct adjacent face carriers" states that no edge parameter interval and no pcurve is assigned without a branch witness or a native parameter incidence that selects one endpoint arc. `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:7144-7159` assigns an interval with neither.

### SN-33. Spine grammar arbitration

**Question.** Which container field distinguishes a standard-nested spine from an FBB-only spine when the file admits both edge-table grammars, or neither?

**Known.** `catia.md` §1 "Detection invariants: a standard file has one nested inner" gives the detection invariants, and the variant table gives an FBB-only spine as a nested container with FBB face rows and `05 08 01` vertices but no standard edge-row table. `crates/cadmpeg-codec-catia/src/container.rs:1263-1274` tries the standard edge-table grammar first, then the FBB-only two-table grammar, and then classifies by the count of `EDGE_DELIMITER` occurrences. The comment at `crates/cadmpeg-codec-catia/src/container.rs:1221-1230` records that this byte sequence does not distinguish the two spines, because FBB-only widths one and three use the standard delimiter. The two probes read different runs: `standard_edge_count` uses `selected_standard_run` and `fbb_only_edge_count` uses `largest_fbb_run`. Nothing shows that the two grammars cannot both admit one stream.

**Need.** We must know the selecting field because the variant fixes the decode route. `crates/cadmpeg-codec-catia/src/families/mod.rs:46-68` applies only the standard route to the standard-nested variant, and applies the standard route and then the freeform route to the FBB-only variant. A spine that is classified standard-nested in error loses the freeform route and transfers no carrier.

**Note.** `catia.md` §1 gives no rule for the state where neither edge-table grammar is admitted. The decoder uses the delimiter count in that state.

### SN-34. Full-form standard endpoint-port identity

**Question.** Does a two-handle row in a full-form standard spine share its endpoint ports with another row in the same table that stores the same handle?

**Known.** `crates/cadmpeg-codec-catia/src/families/standard/fbb.rs:738-746` sets the boundary layout of every parsed row from its handle count alone: a row with two handles is a complete boundary run. `crates/cadmpeg-codec-catia/src/families/standard/fbb.rs:633-645` parses the full-form standard spine with that same row parser, so a full-form row with two handles also carries that layout. `crates/cadmpeg-codec-catia/src/solve/missing_edge.rs:42-59` then gives such a row table-scoped handle identity and gives occurrence-local ports only to rows with more handles. The docstring at `crates/cadmpeg-codec-catia/src/solve/missing_edge.rs:90-93` states the form-level rule, and `crates/cadmpeg-codec-catia/src/solve/missing_edge.rs:1582-1585` selects placement ports only when every row is a complete boundary run, which is also a form-level test.

**Need.** We must know the rule, because a shared port identity collapses two logical vertices before the solver runs. `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:3643-3654` then reduces that edge's candidate domain to one pair.

**Conflict.** `catia.md` §5.4 "Full-form standard `u16be` endpoint integers are not vertex indices or reusable port identities." states that each full-form row contributes two occurrence-local ports even when another row stores the same endpoint integer. `crates/cadmpeg-codec-catia/src/solve/missing_edge.rs:42-59` shares the port identity of a two-handle full-form row through its table scope.

### DI-24. PMI dimension quantity and suffix framing

**Question.** Which field gives the physical quantity of a transferred `Range`/`CstAttr_Dimension` nominal and its deviations, and what do the `B8`, `C1`, and `DC` suffix framings select?

**Known.** `DI-18` records that the semantic subtype of a `Range` production is unknown. `crates/cadmpeg-codec-catia/src/pmi.rs:142-148` gives the length quantity to the nominal and to both deviations of every admitted production. `crates/cadmpeg-ir/src/pmi.rs` defines that quantity as millimetres and defines an angle quantity and a ratio quantity beside it. `crates/cadmpeg-codec-catia/src/pmi.rs:79-88` admits three exact suffix framings. `crates/cadmpeg-codec-catia/src/pmi.rs:101-107` gives all three the same dimension kind, and the neutral annotation record keeps no native reference, so the framing does not reach the neutral model. The transfer reports a coverage count only, and `crates/cadmpeg-codec-catia/src/loss.rs` has no code for it.

**Need.** We must know the quantity to transfer the value. An angular dimension in radians and a length in millimetres are not distinguishable in the current output.

**Note.** `catia.md` §7.1 "A legacy scalar prefix is" resolves a scalar's declared type from its type descriptor and gives `LENGTH` values in millimetres and `ANGLE` values in radians. The `Range` transfer resolves no type and assigns the length quantity to every production.

### DI-25. Compact `1A` operation-class declaration

**Question.** Does a compact self-owned `1A` root declare an operation class?

**Known.** `catia.md` §7.3 "An exact separator-form owner declaration with class name `GSMPlaneAngle`" requires a separator-form declaration with a matching class entry, owner entity, and structural owner for each unresolved operation family node. `crates/cadmpeg-codec-catia/src/design_feature.rs:713-741` also admits a self-owned compact `1A` root that has no structural owner, and reads the operation class name and class entry from that record. Commits on this branch added classes to the admitted operation set, so the compact route now reaches the reference-plane, extrusion, and circular-pattern families.

**Need.** We must know whether the compact root declares a class, because the family node it makes is a neutral feature with a history ordinal.

**Conflict.** `catia.md` §7.3 "All `7C09` records in one graph carrying the same `owner_ref`" states that in compact groups the selected record is an identity anchor and not a class declaration, and that owner class and storage stay unset. `crates/cadmpeg-codec-catia/src/design_feature.rs:728-740` reads the owner class name and class entry from that record.

### SN-35. Same-incidence row endpoint assignment

**Question.** Which serialized relation assigns an endpoint pair to each row when two or more same-incidence rows share one complete bipartite endpoint relation?

**Known.** `catia.md` §5.6 "When more than two vertex rows lie on the same analytic intersection" states that lexicographic ordering does not bind a row, and makes allocation-rank binding a final gauge reduction that follows the mesh constraints. `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:4833-4846` binds the row of rank `k` in the group's serialized order to the vertex row of rank `k` on each sorted side of the relation, and writes that one pair as the row's domain. `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:4935-4938` writes a second such domain for the anchored diagonal stage. `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:3792-3798` makes the reduced set the solver's candidate domain. The retry at `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:3877-3905` searches that same reduced set, so a removed pair does not return. The stage runs only for a line or spline family, a matching sorted face pair, an identical normalized relation, a complete bipartite relation, and a matching boundary frontier.

**Need.** A complete bipartite relation over `n` rows admits `n!` matchings, and the vertex rows are distinct points, so the matchings are not equivalent. A wrong matching gives a wrong line origin, a wrong direction, wrong edge endpoints, and a wrong parameter range. We must know the relation to bind the row.

**Conflict.** `catia.md` §5.6 "When more than two vertex rows lie on the same analytic intersection" applies allocation rank only after the mesh constraints leave an equivalent complete relation. `crates/cadmpeg-codec-catia/src/families/standard/decode.rs:3792-3798` applies it before them.

**Note.** Recovered from a 2026-08-08 audit pass whose ledger commits are reachable from no ref. Its original identifier was `SN-32`, which now names another item.

### SN-36. Allocation-rank binding rule

**Question.** Does allocation rank bind a same-incidence row, and under which condition?

**Known.** `catia.md` §5.6 "When more than two vertex rows lie on the same analytic intersection" states that lexicographic ordering does not bind a row because it is not a serialized endpoint identity, and makes allocation-rank binding a final gauge reduction that follows the mesh constraints. `catia.md` §5.6 "Same-incidence spline or line rows of one curve family" gives rank-binding stages in the same section, and one of them binds the lexicographically ordered pairs of a circle-row relation by equal rank.

**Need.** The two paragraphs give opposite answers for one construct, so the specification does not state a rule that a decoder or a writer can apply. `SN-35` records what the decoder does now.

**Conflict.** The two paragraphs of `catia.md` §5.6 disagree. One removes lexicographic order as a binding relation; the other makes it one.

**Note.** Recovered from a 2026-08-08 audit pass whose ledger commits are reachable from no ref. That pass recorded the disagreement inside its `SN-32` item.

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

**Note.** Reopened by the 2026-08-10 closure audit. Commit `a9117f1d5` labels the scalar as a source-span witness and adds tests that mutate it without changing standalone pcurve geometry. Production extrusion paths use the scalar as a span constraint, a role that corpus records have not yet verified; the tests establish decoder consistency only. Compare the suffix scalar against known source spans in probe-batch parts.

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

**Note.** This item was removed in the tree that added the layout table and the `valid_a8_elided_tail` validator, and it is restored here with a narrower question. A table of observed literal values is a shape test, not the semantics the question asks for. The pole location half of the original question is answered: `catia.md` §6.7 gives the external allocation, and `a5a8::records::a8_surface_from_external_grid` binds it through the pcurve support reference. Reopened by the 2026-08-10 closure audit. Commit `1182d6612` makes the lane labels part of the spec and retains the parsed fields, but the generated-tail tests only repeat those labels. Corpus and probe-batch bytes have not yet verified the range, affine, or extrapolation roles. Vary the surface parameter range in a probe batch and read which tail lanes change.

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

**Note.** Reopened by the 2026-08-10 closure audit. Commit `403bd91cb` derives labels from tuple positions and arithmetic and tests only synthetic records. It does not bind those labels to the native `0638`/`2569` topology or `0x21xx`/support namespaces. The current spec promotes those labels without an independent byte witness.

### ZE-05. Zero-entity ownership-root selection

**Question.** How does a zero-entity stream select its ownership root when more than one face-roster/shell/body triple is structurally valid?

**Known.** `zero_entity_ownership_roots` recognizes every contiguous `[0x6142, 0x6006, 0x6508]` triple with checked fields and retains the candidates in source order. The neutral zero-entity route binds ownership only when exactly one candidate exists. Multiple candidates remain native and do not select a body or shell.

**Need.** We must know the terminal/root identity rule or reject multiple valid ownership triples before transferring body and shell ownership.

**Note.** The decoder now rejects ambiguous ownership for neutral transfer and retains every exact candidate. The source rule that identifies one terminal root when multiple complete triples exist remains unknown.

## 6. E5 `0D 03`

### E5-01. `0xa0` circle branch

**Question.** Which field selects the circle branch of an `0xa0` wrapper?

**Known.** `catia.md` §9 "Framing `E5 0D 03 <cls> <sub> <payload_size_u16le> 00 00 00 <record_id_u32le> <payload>`, stride" and `catia.md` §9 "Classes: `0x01` body" defines the admitted `0xa0` wrapper and analytic circle primitive.

**Need.** We must know the selector to choose the correct circle arc.

**Note.** Reopened by the 2026-08-10 closure audit. Commit `0bb127ce2` adds a cone jet normalization test but no byte-level selector evidence for the circle branch. A generated cone chart exercising the decoder does not identify which wrapper field selects a circle. Read the `0xa0` wrapper fields of corpus and probe-batch parts that contain known circles to fix the selector.

### E5-02. `0xa0` co-parametric mapping

**Question.** What is the general parameter mapping from an `0xa0` wrapper to its primitive?

**Known.** For the cone subset, `q_circle = (R/ca_q_scale) * q_ca`.

**Need.** We must know the other mappings to trim the primitive correctly.

**Note.** Reopened by the 2026-08-10 closure audit. Commit `0bb127ce2` proves only the cone subset in a synthetic `E5Surface`/`E5Pcurve::Jet` fixture. It does not establish the mapping for other carriers or the native wrapper parameter relation.

### E5-03. Plane-cap digon orientation

**Question.** Which fields orient a plane-cap digon?

**Known.** `catia.md` §9 "**E5 orientation** is" defines the E5 incidence graph and the non-degenerate orientation equations.

**Need.** We must know the fields to orient the cap when its boundary is a digon.

**Note.** Reopened by the 2026-08-10 closure audit. Commit `acb22c30f` adds a plane-digon orientation hint based on degree-5 jets, carrier classes, support ranges, and synthetic orientation tests, then promotes that relation to the spec. The test fixture is generated from the same rule; corpus CATIA bytes have not yet verified that these fields are the native global-sense anchor. Build a probe batch with plane caps of known sense and compare the orientation fields.

### E5-04. Rank-deficient plane frame

**Question.** How is a rank-deficient E5 plane frame completed?

**Known.** The decoder retains the stored frame lanes. The general frame equation requires independent axes.

**Need.** We must know the completion rule to construct the plane.

**Note.** Reopened by the 2026-08-10 closure audit. Commit `66545be34` removes the item and records the rank-one normal/sign procedure only in coverage documentation. The documented normal and first-nonzero-component sign are decoder choices. Derive the completion rule from the rank-deficient plane frames in corpus and probe-batch parts.

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

### E5-12. E5 occurrence-side fallback

**Question.** Which intersection side is authoritative when an E5 edge has zero, one, or conflicting occurrence-side candidates?

**Known.** An exact two-side intersection context requires exactly two sides with matching parameter ranges. When no such context exists, the decoder retains a surface curve only for exactly one resolved side. Multiple sides with no exact context remain unbound; source order does not select one.

**Need.** We must distinguish a valid one-sided construction from multiple or conflicting sides before selecting an edge carrier.

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

**Known.** `catia.md` §6.7 "**Object-stream topology:**" fixes loop membership. `catia.md` §6.6 "The elided-pole form places a fixed 141-byte" gives the external pole allocation, and `a5a8::records::a8_surface_from_external_grid` binds it when its byte length is unique in the stream. A carrier that keeps no grid stays an identity-bearing surface node, so a class-`21` pcurve on it lifts to no 3D endpoint, no `5e` edge on it takes a vertex locus, its `62` loop fails the endpoint conjunct, and the owning `5f` face leaves the connected graph.

**Need.** We must know whether the external allocation transfers the face, and we must hold the exclusion when it does not. Transferring the face without a grid needs invented vertex coordinates.

**Note.** This item was removed by the tree that made the elided-tail test stricter. A stricter test admits fewer carriers, so it cannot have transferred a face that was excluded before. No test and no decode-report count shows the face reaching the neutral model, and the tree's commit message states no rationale. The item is restored until one of the two exists. The remaining carrier-program semantics are tracked by OS-15. Reopened by the 2026-08-10 closure audit. Commit `1de8cc061` adds an end-to-end synthetic A8/grid/B5-chain fixture and validates one generated triangle. That proves one decoder-constructed path, not the native external-grid ownership rule or general face transfer.

### FV-10. Float-packed fixture validity

**Question.** What object-stream and analytic-carrier records must a synthesized float-packed inner-no-FBB input contain to be a valid specimen of the variant?

**Known.** `catia.md` §6.7 "For `b5 03 29`, the 185-byte payload is" fixes the cone chart and §6.7 "`b5 03 5d` (vertex identity)" fixes the native vertex-identity chain. The committed golden fixtures for this variant satisfy neither and reach no geometry path, so no golden fixture exercises the object-stream transfer route. Route coverage is a programmatically synthesized object stream held in the crate's tests, which decodes end to end through the container.

**Need.** We must know the minimum valid record set to synthesize golden fixtures that hold the object-stream route under snapshot.

**Note.** Loop membership no longer limits this item. `catia.md` §6.7 "**Object-stream topology:**" fixes the rule a fixture must satisfy: each `62` node is named by exactly one `5f` face, its trailing reference is that face's carrier, and its `n_refs` equals `2*edge_count+1`. The remaining work is the `5d` identity chain: build a `5d`, a class-`05` roster, class-`06` parameter incidences, and `05 08 01` vertex rows that agree with the lifted pcurve endpoints of every incident edge. A fixture that omits the chain reaches the excluded state for a carrier without an endpoint source and proves nothing about the route. Reopened by the 2026-08-10 closure audit. Commit `0daf2d957` proves that one generated fixture is sufficient for one triangle. It does not establish the minimum or necessary record set, nor does it validate the existing golden fixtures.

## 8. Appearance

### AP-01. `FeatureForColor` face selection

**Question.** How does `SelectingFeatureForColorUuid` select the face targeted by an `EC 03 R G B A` override that occurs with an `EB 01 R G B` all-face color?

**Known.** `catia.md` defines both color packets. The `EB` value applies to the complete face population. The `EC` value supplies the override color asset. The positional FBB rows independently store the effective face colors, so neutral face appearance binding does not depend on this UUID incidence. The application object graph contains `FeatureForColor` and `SelectingFeatureForColorUuid`, but the UUID-to-standard-face incidence is not assigned.

**Need.** We must know the incidence to preserve or write the native selection relation independently of the effective FBB presentation population.
