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

## 2. Design intent

### DI-01. Compact schema-program semantics

**Question.** Which record boundaries and operations occur in a compact schema program?

**Known.** `catia.md` §7.1 "A legacy schema catalog opens with" defines the program's outer boundary. The decoder retains the complete span and identifier candidates. The candidate scan does not establish records or operations inside that span. Identity intervals and typed fields before the catalog opener are separate structures.

**Need.** A CATIA-authored compact program and an independent behavior check must establish its record boundaries and operations before semantic transfer.

**Note.** The 79-file reference population contains zero exact legacy catalog openers. Its 319 plain `CATCatalogManager` strings do not establish that production. Constructed `Foo` fixtures test opaque retention and identifier detection only.

### DI-02. Unresolved schema-role selectors

**Question.** What field role does each unresolved schema-selector byte select?

**Known.** `catia.md` §7.1 "Within an identity interval, `<inclusive-length:u8> <name:utf8> <selector>`" defines the byte production and its target selector. A literal role name assigns its role directly. An unresolved selector byte does not.

**Need.** A real legacy specimen and a schema-local role mapping must identify the selected role before assigning field semantics.

**Note.** The 79-file reference population has no required legacy catalog opener and therefore no legacy role-selector population. The selector bytes in constructed framing tests remain unnamed; their following field codes do not supply a role mapping.

### DI-03. Part-container object-graph ownership

**Question.** How does an outer `Data` declaration's 16-byte identifier select the physical stream containing the part's design-history object graph?

**Known.** `catia.md` §3.1 "The outer logical stream named `Data`" defines concrete/base class declarations and their identifiers. `CATFeatCont` names a base class of `CATPrtCont`, directly or through `CATProdCont`. This class ancestry does not join object graphs. `catia.md` §7.3 "An object graph is preceded by" defines object-graph framing, entity identity, structural owner groups, and source-schema selection. Class names, structural owner groups, and physical stream ownership are distinct incidences.

**Need.** We must know the identifier-to-stream mapping across directory name forms and establish unique complete graph-extent containment before assigning the part's modeling scope.

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

**Question.** Does the trailing `d` incidence of a lead-`54` compound relation-program frame select its result when the selected class is `paramout`?

**Known.** `catia.md` §7.3 "A compact compound relation-program instance is" and `catia.md` §7.3 "A separator-form compound relation-program instance is" defines lead-`12` and lead-`54` relation-program frames. Lead-`12` selects `ref(h)` at decoded field 13 as its result when the exact same-graph class is `paramout`. The program identity, repeated reference, context identity, and lead-`54` trailing identity remain distinct incidences. The lead-`54` `d` atom is decoded field 15. A complete typed expression and its source symbols give the ordered inputs.

**Need.** We must establish the lead-`54` result incidence independently of its current class predicate before transferring a separator-form result.

**Note.** The 79-file byte census has nine lead-`12` frames whose `h` selects entity 272, class `paramout`, and three lead-`54` frames whose `d` selects entities 60, 52, and 100, class `Volume`. No other incidence in those frames selects `paramout`. The existing positive lead-`54` fixture is synthetic; the real frames establish its trailing incidence and rejection of `Volume`, but not a positive result assignment.

### DI-08. Legacy relation `body` selector

**Question.** What identity space does a nonlocal `body` selector on a legacy typed relation use?

**Known.** `catia.md` §7.1 "A typed relation consists of" defines the local identity case. A `body` selector equal to the containing identity supplies the local relation context.

**Need.** We must resolve a nonlocal selector to bind the relation context.

**Note.** None of the 79 retained CATIA files contains the required legacy catalog opener. The local self-identity rule and synthetic nonlocal rejection do not establish the nonlocal selector namespace.

### DI-09. Legacy relation `param` selector

**Question.** What identity space does a nonlocal `param` selector on a legacy typed relation use?

**Known.** `catia.md` §7.1 "A typed relation consists of" defines the local identity case. A `param` selector inside the containing run selects the relation parameter only when the `body` selector equals the relation's containing identity.

**Need.** We must resolve a nonlocal selector to bind the relation result.

**Note.** None of the 79 retained CATIA files contains the required legacy catalog opener. A synthetic same-run binding or nonlocal rejection does not establish an external parameter identity.

### DI-13. Active configuration state

**Question.** Which production defines document design configurations, and which field selects the active state?

**Known.** `catia.md` §7.3 "A self-defining schema-configuration record is" and `catia.md` §7.3 "A schema-configuration-row link is" defines schema-local `Configuration` records, `configrow` successor chains, selected value schemas, and the source-ordered open intervals between rows. These productions do not define document design configurations or assign active state. `Configuration`, `configrow`, and `DesignTable` catalog entries can remain unselected by every object record and value record; catalog vocabulary alone does not establish an instance production. `catia.md` §7.3 "Metric-thread standards state has a separate object production" defines the standards-table ownership of `StandardType`, `SensThread`, and `TypeThread` state. A terminal `active` atom three in that production belongs to the standards object. Selected hole enumerations, metric-thread sheets, and reference caches have separate field identities.

**Need.** We must identify a complete document design-table relation, its model-parameter column associations, and its integer configuration-row selector before we can transfer document configuration identities or active state.

### DI-17. `PRTSketch` and `Sketch` payload roles

**Question.** Which contextual productions assign roles to the remaining atoms and standalone reference vectors in `PRTSketch` and `Sketch` fields?

**Known.** `catia.md` §7.3 "`PRTSketch` and `Sketch` select field schemas" separates the field schema from the generic payload grammar. Only the complete `Sketch` owner declaration assigns the initial list a member-roster role. Standalone runs retain separate native incidences, including count-coherent runs and targets repeated from the roster. A class name or count-complete list alone does not assign sketch identity or geometry.

**Need.** We must identify complete contextual productions before assigning neutral roles to the remaining atoms and standalone vectors.

### DI-18. Range semantic owner

**Question.** Which sketch constraint, feature/model-tolerance object, or product-manufacturing-information object owns each complete schema-selected `Range` interval?

**Known.** `catia.md` §7.3 "A catalog-resolved value selection named `Range`" defines the complete interval independently of selector cardinality and suffix framing and retains every incoming payload reference and storage selector. Range intervals occur with no slots or with finite and unset lower and upper tolerance-deviation slots. They occur in `Range`-only, `Range`/`CstAttr_Dimension`, and larger selector sequences. Exact `D8`/`81 93`, `D8`/`81 DB`, and `DC`/`81 DB` scalar suffix dialects supply the nominal independently of selector cardinality. A `Range`-only interval can carry both deviations and its nominal. Range intervals can occupy a stable `MechanicalPart` aggregate slot selected by a `_SpecList`/`FeatureFSUR` relation whose other operand carries `UserPattern` and `SimpleLimit`; sketch-versus-PMI is not an exhaustive ownership choice. Their paired and incoming classes also include presentation, TPS capture-link, limiting-element, Boolean, geometry, and catalog-manager classes. Some intervals have no incoming incidence, one incidence, duplicate references from one object, or references from multiple objects. The narrower two-selector constraint-range production retains five exact selector/code/trailer tuples. `ListAggregator` references can include unrelated and repeated identities. A shared structural design-object owner can contain feature, geometry, and presentation classes. These incidences do not distinguish sketch, feature/model-tolerance, and product-manufacturing-information ownership. A two-selector range transfers as one opaque sketch constraint only when exactly one total incoming incidence resolves to its same-graph paired source entity and object record, and that source object's complete owner chain reaches one transferred `Sketch` before another transferred feature. The source object record is retained as one unresolved native operand. Its nominal, deviations, evaluation, and suffix framing remain native because this production does not assign a physical quantity. Other two-selector ranges remain unresolved. The `T` definition with ordered `ProfileList`/`GSMProject`/`Range` selectors has the feature-local diameter frame and cylinder-face incidence defined in §7.3.

**Note.** A Range-only entity with the exact source-closed production in `catia.md` §7.3 transfers as a targetless PMI size dimension. `DiameterThread` selects diameter and `FeatureRSUR` selects general feature size; the finite nominal and both finite deviations are millimetre lengths. Range-only entities outside those two definitions remain unresolved. The feature-local GSMProject incidence does not establish a PMI annotation owner.

**Need.** We must identify the owning incidence, semantic subtype, and target for the remaining Range-only and larger-selector productions. For the feature-local GSMProject diameter attributes, we must identify the exact aggregation and presentation-owner incidence and any native PMI deviation production. Range-only PMI dimensions remain targetless until their target grammar is established.

### DI-19. Sketch placement

**Question.** Which exact incidence selects a sketch support face or surface? Which complete value productions encode placements outside the nine-control frame program?

**Known.** `catia.md` §7.3 defines the first-roster-item placement join, its containing field group, and the horizontal-axis, vertical-axis, and origin controls. Empty `Position`, `AbsoluteAxis`, and `_Supports` fields carry no frame or support incidence. The model-space frame does not itself identify a support face.

**Need.** We must identify a complete support incidence and the remaining placement productions.

### DI-20. Sketch construction state and profile membership

**Question.** Which fields mark construction geometry? Which relation assigns sketch geometry to a profile?

**Known.** `catia.md` §7.3 defines sketch member identity, ordered local point coordinates, and placement independently. None assigns standard or construction state. Empty `OutputGeometries` fields and solver lists do not assign profile membership. The native object and entity records retain the geometry and its exact incidences.

**Need.** We must identify the complete geometry-owned construction-state production, including both standard and construction states, and the exact native profile-membership incidence. Closed profiles require standard curves and exact endpoint identities; point coordinates and geometric proximity do not establish that relation.

### DI-22. Sketch constraints

**Question.** What are the operand and value fields of each dimensional and non-dimensional sketch constraint?

**Known.** Constraint-range values and structural incidences are retained separately. An exact `2DPoint` field-to-`ConstraintDYS` reference retains both native field and entity identities, offsets, and outbound reference occurrences. This incidence assigns no neutral sketch geometry or constraint type. A structurally proven sketch-owned `Range`/`CstAttr_Dimension` constraint retains its exact nominal and interval values natively. These selectors and values alone do not assign a physical quantity, neutral parameter, dimensional subtype, driving or active state, or solver semantics.

**Need.** We must identify the complete type production and its required ordered geometric operands, the dimensionality and units of valued constraints, and the separate driving and activity state productions. Each assignment requires exact source identities and independently calibrated values.

### DI-23. Feature-instance grammar

**Question.** How do operation fields, definition-bound values, and structurally owned operand objects form one feature instance?

**Known.** `catia.md` §7.3 "All `7C09` records in one graph carrying the same `owner_ref`" defines each incidence independently. Paired entity tables admit opaque childless object records and preserve their complete bodies, but do not assign roles inside those bodies. A complete two-definition value chain with a supported second role transfers one typed parameter, but it does not assign that parameter to an operation role. Operation-named field records can share one class identity across several structural owner groups. A `Hole` class cohort contains one empty self-classified record plus list, atom-vector, mixed, or empty records under other owners. The shared class identity does not establish whether the cohort is a schema object, one feature instance, or a field program reused by multiple instances. An operation-named field class or field vocabulary does not assign feature identity, operands, outputs, or replay order. An exact separator-form owner declaration for `GSMPlaneAngle` or `GSMPlaneOffset`, with matching class entry, owner entity, and structural owner, establishes an unresolved constructed-reference-plane family node; support, angle, signed offset, normal, in-plane frame, and construction dependencies remain unresolved. A separator-form owner declaration gives structural ownership and class identity. `Prism_EndLimit_Length`, `Prism_ThickThin1`, `Prism_ThickThin2`, `Revol_ThickThin1`, `Sweep_ThickThin1`, `EdgeFillet`, and `CircPattern_RadialNumber` can label schema groups; the class and complete structural incidence do not establish an operation instance. Definition-bound values, ranges, and field relations remain native records. A typed parameter retains its value without an operation role or an owner inferred from these labels. An exact payload reference from a transferred feature object's field to a different transferred feature, or to a target whose complete owner-design-object chain reaches that feature, with an earlier feature ordinal transfers one deduplicated structural dependency in relation order. Storage selectors, unresolved targets, incomplete or cyclic owner chains, self-links, and forward targets do not. This structural edge does not identify an operation-specific input role.

**Need.** We must identify the native operation-instance production, its exact profile or geometric operands, typed values, result identities, and operation-specific dependencies. Each family requires an independently calibrated source instance and regeneration order.

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

**Known.** `catia.md` §6.2 "Frames an explicit rolling-ball surface jet." defines the A5 header as one canonical width-1 compact integer (`4n+1`, `0 ≤ n ≤ 63`). The header value is retained, but no selector role is assigned.

**Need.** We must know the selection rule to write the correct token.

### SN-02. `a5 03 32` numeric continuation

**Question.** What fields follow the three aligned jet blocks in each numeric-continuation length class?

**Known.** `catia.md` §6.2 "Frames an explicit rolling-ball surface jet." defines the knots, the `0x0c` or `0x08 <4n+1>` array marker, and the value, first-derivative, and second-derivative blocks. The continuation after those blocks has more than one length class and remains untyped.

**Need.** We must know its lanes and terminal fields to read and write the complete record.

### SN-03. Width-coded class-`0x5e` header token

**Question.** What does the width-coded header token of a `b2`, `b3`, or `b4 03 5e` record select?

**Known.** The record framing gives the token width and value.

**Need.** We must know the selector namespace to interpret and write the edge record.

### SN-04. Class-`0x5e` terminal byte

**Question.** What does the terminal byte of a `b2`, `b3`, or `b4 03 5e` record control?

**Known.** The width-coded terminal is a single-byte `allocation_ref` with the seven admitted value/form pairs `(0,backward_distance)`, `(0,selector2)`, `(8,backward_distance)`, `(8,selector2)`, `(9,backward_distance)`, `(10,backward_distance)`, and `(10,selector2)`, encoded as `01`, `02`, `21`, `22`, `25`, `29`, and `2a`. The object-stream record additionally admits `(9,selector2)`, encoded as `26`. The raw byte and decoded value/form are retained independently of the endpoint identities, and the terminal exhausts the payload after the five references.

**Need.** We must determine what the decoded value means, whether it has any writer-level relation to a successor definition or carrier construction, and whether the four-reference width-coded variant has the same role. Do not use the local successor-layout correlation as a universal endpoint-orientation rule.

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

**Known.** `catia.md` §6.3 "When the `op1` support identity equals" defines the exact unique class-`0x19` binding, the unique embedded type-`3` cylinder binding, and the unique standard `00 33` carrier binding. Before the standard-carrier lookup, §7.5 outer alias rows canonicalize a raw `op1` tag only when exactly one retained alias row supplies one canonical surface-roster tag. Direct surface-support rows preserve their tag; grouped non-surface rows require one direct row in the same exact group. Duplicate raw tags, duplicate group targets, missing group targets, unknown geometry, and failed endpoint lifts remain unresolved. B5 object-stream references and allocation operands remain separate namespaces.

**Need.** We must resolve the remaining persistent identities that do not select a unique admitted class-`0x19`, embedded-cylinder, standard-carrier, or canonical-alias target, including any identity-bearing carrier family not covered by an explicit chart or endpoint witness.

### SN-14. Incomplete multiple-FBB population membership

**Question.** Which serialized relation assigns an FBB layout to a surface/support population when the source-closed FBB and surface/support lanes do not have equal population counts or local cardinalities?

**Known.** `catia.md` §5.1 "trim_record[i] -> face_outer_bound_row[i] -> face i" defines topology reconstruction for one positional spine. A source-closed FBB layout and a source-closed surface/support population form an admitted positional membership pair when the two lanes have equal population counts and each paired position has equal face and edge cardinalities. The decoder preserves each pair's local namespaces and emits every admitted body and shell. Repeated face/edge/vertex keys do not create ambiguity when this lane relation closes.

`standard_surface_populations` retains every contiguous surface-record chain ending at a `0x60` support table, and the FBB walk retains every population with a complete trim, edge-table, and counted-vertex layout. The source-closed lanes are paired in source order only after their lane count and every local face and edge cardinality agree. A repeated structural key does not select by itself and does not prevent a closed lane relation.

**Need.** We must identify the independent membership relation for an unmatched source-closed layout or surface/support population so its topology can be emitted without assigning a carrier or support table from another population.

**Conflict.** A positional pair is not sound when one lane is incomplete or a paired position has different local face or edge cardinality. Counts from a single lane and allocation order do not provide that missing membership witness.

**Note.** The admitted positional relation is source order within the two complete source-closed lanes. It is not a persistent allocation-order join and does not select an entry from a partial lane.

### SN-16. Class-`0x20` persistent reference

**Question.** How does the `op1` or persistent-tag reference in an `a5 03 20` record select a serialized record?

**Known.** `catia.md` §6.3 "`b2 03 20` is the B-family form" defines the pcurve payload and support binding for exact identity and chart matches.

**Need.** We must resolve other references to bind the pcurve support.

### SN-17. Class-`0x62` owner bounds

**Question.** Which serialized relation supplies the binary64 rectangle for a fixed-nine owner without a source-closed carrier chart, and what non-identity role do the binary32 lanes have in tagged alternating and width-coded packets?

**Known.** In the all-compact grammar, the three binary32 pairs are the model-space X, Y, and Z bounds of the owned face boundary. In tagged alternating packets with an established NURBS-carrier relation, the binary64 pairs are the carrier parameter rectangle and may cover a proper subdomain. The source-closed owner carrier chart in `catia.md` §6.4 also proves that the binary64 pairs are the carrier parameter rectangle for its fixed-nine packet, independent of the fixed-nine identity dialect. A release sweep of 78 inputs decoded 14,646 owner packets: 951 fixed-nine packets (732 all-compact, 112 tagged-u16-strong, and 107 width-coded-strong) and 13,695 counted packets. The fixed-nine chart relation occurred 45 times: 15 all-compact, 30 width-coded, and zero tagged. A representative width-coded chart has owner, carrier, bridge, and parameter-point record offsets `603047`, `602679`, `602884`, and `602923/602954/602985/603016`; its binary64 rectangle is closed by those four parameter points. The tagged binary32 lane is not a direct face-boundary box. Width-coded binary32 bounds can enclose more than one face boundary. The standard face-boundary candidate predicate therefore applies only to all-compact owners; tagged alternating and width-coded lanes do not supply that predicate.

**Need.** We must identify a framed carrier or allocation relation for fixed-nine packets without an owner chart, and define any further binary32 role that does not assert a face identity for tagged or width-coded packets.

**Conflict.** The tagged owner at record offset `40482` has a binary32 envelope that contains seven neutral face bounding boxes in the corresponding sibling topology; the same comparison contains tagged envelopes with six and five face boxes. Width-coded envelopes in another sibling comparison each contain three face bounding boxes. Binary64-only parameter-domain containment for the tagged rectangle at offset `40482` selects multiple NURBS carriers. These observations reject face binding by envelope containment or by numeric parameter-domain shape.

**Note.** Fixed-nine packets without charts retain typed identity encodings, numeric tails, and source-local identity targets. Representative width-coded packets at record offsets `597097`, `599050`, and `656726` have resolved identity targets but no owner chart. No allocation rank, adjacency, or geometric shape rule may promote those packets. The current decoder preserves the lanes and withholds the unresolved bindings.

### SN-18. Class-`0x62` owner-to-face binding

**Question.** Which field binds a fixed `b2`, `b3`, or `b4 03 62` owner packet to its face record?

**Known.** A complete class-`0x5f` node immediately preceding a fixed-nine owner binds to that owner when its target's checked successor is the ninth owner identity. Terminal `03 05` admits every fixed-nine grammar. Terminal `03 03` admits the all-compact grammar only. The node target and owner identities are allocation-local. Every compact fixed-nine identity retains its wire addressing form. Backward-distance identities resolve within the packet's contiguous local class-`0x5d`/`0x5e` allocation sequence; the exact class-`0x65` separator payload `81 03 05 0d` starts a new allocation sequence, and raw weak identities do not enter that walk. The source-closed carrier/bridge/four-side/owner production binds the owner packet to one carrier chart and proves its complete parameter rectangle. The five-reference class-`0x37` bridge is a supported-surface construction that names the carrier surface, two support surfaces, two support pcurves, six controls, and construction radius. A width-coded support value selects a unique outer alias row by persistent tag and inherits that row's canonical surface tag; other addressing forms remain allocation-local. The eight-reference A-family bridge remains structurally retained. This relation does not assign a standard face ordinal.

**Need.** We must assign the eight-reference bridge and identify the allocation-group relation that maps the supported-surface identities or bound node target to a standard face ordinal without comparing local identities across groups.

**Note.** The all-compact model-space box can nominate one geometric face, but this geometric witness does not define the source identity join. A count-framed `03 62` packet has a variable allocation-reference lane and an opaque nonempty tail; it is not a fixed-nine packet with the admitted numeric rectangle and model-space bounds. Its adjacent `03 5f` successor relation names the counted owner packet only. It does not join a standalone `a5 03 34` frame or assign a standard face ordinal. Control-net containment and boundary coincidence remain geometric candidates, not allocation identity.

### SN-20. Class-`0x18` parameter-point selectors

**Question.** What does each of the four prefix selectors in a `b2`, `b3`, or `b4 03 18` parameter-point record select?

**Known.** The decoder retains the four selectors and the parameter-point payload. In the source-closed owner carrier chart, selectors `05` and `09` are opposing constant-parameter sides, and selectors `0d` and `11` contain the lower and upper orthogonal bounds. `B:28` uses constant first-parameter sides. `B:2b` and `A:32` use constant second-parameter sides. `catia.md` §6.4 defines the exact scalar relation and zero elision.

**Need.** We must define these selectors outside the owner carrier chart and assign the remaining side-record scalars.

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

**Known.** A structurally complete class-`0x5f` face node retains either its compact or tagged-`u16` target encoding and its two terminal bytes. The `03 05` terminal form retains a derived relation to an immediately adjacent class-`0x62` packet when the checked successor identity matches the packet's final reference. The `03 03` terminal form retains the same relation for an all-compact fixed-nine packet. Other terminal and packet-grammar combinations remain unassigned. A separate source-closed production proves that some fixed-nine packets terminate a support-bound carrier chart. `catia.md` §6.4 defines the independent fixed-nine owner-to-A5-carrier and standard-face matching relation. It assigns the packet as a face-local carrier chart when every complete face-owner matching induces the same carrier, but it does not assign the adjacent class-`0x5f` node itself.

**Need.** We must identify the class-`0x5f` node's role for relations that do not close through the fixed-nine carrier-and-face matching.

### SN-26. Revolution profile allocation identity

**Question.** How does the `u16le` profile allocation identity in a `b2 03 2d` revolution record select its native profile record?

**Known.** `catia.md` §5.15 "The `00 33 30` byte is only" defines the axis frame, angular chart, and exact unique profile-interval binding.

**Need.** We must define the allocation namespace and directrix relation when profile intervals do not identify one record.

### SN-27. Consolidated class-`0x27` plane carrier

**Question.** What semantic roles do the four tail scalars of an `ec` class-`0x27` record have, and what supplies its missing plane axis frame?

**Known.** A complete `b2`, `b3`, or `b4 03 27` frame has payload marker `b4`, a selector, and a nonempty finite f64 lane. Selector `e4` carries seven values as a two-coordinate point, a two-coordinate direction with an omitted third component, and a three-scalar tail. Selector `c4` carries eight values as a two-coordinate point, a three-coordinate direction, and a three-scalar tail. Selector `ec` carries six values as a two-coordinate point and a four-scalar tail. Other selectors retain their complete finite scalar lanes without a neutral plane layout. The direction-bearing `e4` and `c4` layouts define a plane frame with origin `(point_x,point_y,0)`, the stored unit direction as `u_axis`, global Z as the second chart axis, and `unit(u_axis×Z)` as the normal. Their tails are finite with a positive first scalar and an increasing final pair. A direction-bearing carrier binds a consolidated pcurve side only when endpoint lifts select exactly one carrier and reach the object-stream vertices. A directionless `ec` carrier is retained in the native namespace and does not supply a neutral plane support.

**Need.** Resolve the `ec` tail roles and its axis-frame source before transferring that layout as a neutral plane support.

### SN-37. Fixed-nine owner boundary cycle

**Question.** Which fixed-nine references form a face-boundary incidence witness, and what identity does that witness establish?

**Known.** A fixed-nine packet exposes a boundary-cycle relation only when exactly four backward-resolved targets are class `0x5e`, every target has two resolved class `0x5d` endpoint records, and the four endpoint pairs form one simple four-vertex cycle. The relation retains the four identity slots, target offsets, and endpoint offsets in slot order. A source-scoped class-`0x5f` boundary prelude is retained when it immediately precedes the first cycle target, its contiguous span to the owner contains only class `0x5d` and `0x5e` records with exactly four class `0x5e` records, its terminal pair is `27 03` or `27 05`, and its target equals the owner's ninth identity plus `10`. These relations are scoped to the packet's bounded allocation source and do not assign a standard face ordinal. A release witness sweep over 54 CATPart inputs completed 53 with status 0 and one timed out; exactly two successful inputs contained one closed four-edge cycle each. In the first witness, the owner is at source-relative offset `61352`, the face node at `61235`, and the four class-`0x5e` targets are at `61248`, `61283`, `61318`, and `61335`; their class-`0x5d` endpoint records are at `61265`, `61274`, `61300`, and `61309`. In the second witness, the owner is at `40482`, the face node at `40365`, and the four targets are at `40378`, `40413`, `40448`, and `40465`; their endpoint records are at `40395`, `40404`, `40430`, and `40439`. In both witnesses the cycle references, owner references, and endpoint network remain in the same bounded source. The source-scoped scan has no intersection between cycle references and standard support tags, and the B5 reference walks produce no cycle-to-standard hit. The current release standard-topology route transfers the control populations used for the face-domain comparison: two populations retain no open alternate second-face domain, and one retains 52 repeated rows whose allowed second-face domains are singleton before the joint solver. A neutral sibling comparison confirms compatible boundary signatures where split cardinalities match, while another sibling has different split cardinalities; neither identifies a cross-source allocation identity. Other fixed-nine roles, including packets with six disjoint edge targets or mixed `0x5d` and `0x5e` targets, remain unclassified.

**Need.** Map the retained source-scoped face node and closed owner-boundary cycle to the standard face and edge-support records through an allocation-scoped relation. Do not compare local endpoint, owner, or curve identities across bounded record sources.

**Conflict.** Class-`0x5b` and class-`0x5c` records occur in the cycle source and carry bounded control lanes, but no admitted field in those records joins a cycle identity to a standard support tag. Two closed cycle witnesses contain adjacent `b2 13 5b <length> 05 ...` and `b2 03 5c <length> 05 ...` frames, where `05` is the width-coded header token, at file offsets `61085`/`61121` and `40217`/`40251`; their source offsets are `13718`/`13754` and `7580`/`7614`. A corrected release audit over 53 successful inputs retained 824 complete control records, comprising 557 class-`0x5b` and 267 class-`0x5c` records. The cycle-adjacent length/header shapes recur outside the cycles. Their retained fields are source index, source offset, frame width, flag, class, header token, and complete payload only. Treating those lanes as an allocation bridge would add an unsupported relation.

**Note.** The fixed-nine cycle and the complete class-`0x5b`/`0x5c` control frames are retained as native source-scoped evidence. The independent neutral sibling and control-record audits do not close the allocation boundary. No production join or joint-solver input change is justified until a paired record relation crosses the source boundary.

### DI-24. PMI dimension quantity and suffix framing

**Question.** Which field gives the physical quantity of a `Range`/`CstAttr_Dimension` nominal and its deviations, and what do the `B8`, `C1`, `DC`, and `DF` suffix framings select?

**Known.** `catia.md` §7.3 defines the interval, nominal, constraint, suffix-framing, and owner-incidence productions. `B8` and `C1` retain a constraint evaluation without a Range nominal; `DC`/`81 DB` and `DF`/`81 92` also supply the Range nominal production. The same scalar and deviation program can use either latter framing. None assigns length, angle, or ratio semantics. A source-closed sketch incidence can retain an opaque constraint; the scalar and deviations remain untyped native values. No neutral PMI dimension or sketch parameter is emitted from this production. `attributes.dimension-quantity-unresolved` reports each finite scalar withheld for this reason. Range-only `DiameterThread` and `FeatureRSUR` productions are separate source-closed size dimensions whose definitions establish length semantics. `DI-18` retains the separate feature-local `GSMProject` millimetre-diameter incidence and the remaining Range ownership questions.

**Need.** We must identify the serialized quantity source and its join to the constraint-range entity before transferring the scalar and deviations as length, angle, or ratio values.

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

### FV-09. External pole-grid ownership

**Question.** Which identity relation binds an elided-pole `a8 03 34` carrier to its external pole grid?

**Known.** `catia.md` §6.6 defines the elided-pole carrier and external pole allocation. Object-stream topology independently defines pcurve, edge, loop, and face ownership.

**Need.** We must define the grid identity and scope that bind the carrier, its pcurves, and its topology to one external pole allocation.

### FV-10. Float-packed fixture validity

**Question.** What object-stream and analytic-carrier records must a synthesized float-packed inner-no-FBB input contain to be a valid specimen of the variant?

**Known.** `catia.md` §6.7 "For `b5 03 29`, the 185-byte payload is" fixes the cone chart and §6.7 "`b5 03 5d` (vertex identity)" fixes the native vertex-identity chain. The committed freeform fixtures include geometry-transferred A5 and A8 carriers, but they do not exercise the object-stream transfer route: that route has no face, loop, edge, coedge, or vertex population in those fixtures. Route coverage is a programmatically synthesized object stream held in the crate's tests, which decodes end to end through the container.

**Need.** We must know the minimum valid record set to synthesize golden fixtures that hold the object-stream route under snapshot.

## 8. Appearance

### AP-01. `SelectingFeatureForColorUuid` presentation target

**Question.** Which presentation object does `SelectingFeatureForColorUuid` select?

**Known.** `catia.md` §7.4 "A `FeatureForColor` schema selection can carry" defines the color, selector atom, and UUID-named entry as distinct source values. Their framing does not assign a B-rep face. Governing FBB rows independently supply effective face colors.

**Need.** An independent presentation-object incidence must establish the selector's target namespace. A face-selector rule must identify a native face independently before binding its appearance.

**Note.** The four reference `FeatureForColor` groups in CTC-05 and STC-10 carry black-alpha-127 assets, selector atom `1`, and UUID-named atom `12`. Their 434 governing FBB rows are all pale blue. The sibling AP242 black styles reach 20 annotation-curve occurrences and 68 tessellated-annotation occurrences, respectively, and zero advanced faces. These groups supply no face-selector witness.
