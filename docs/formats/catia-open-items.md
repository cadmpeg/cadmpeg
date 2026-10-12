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

### DI-05. Compact inline schema selectors

**Question.** Which schema entries do the compact inline tokens `F2`, `F0`, and `F1` select?

**Known.** `catia.md` §7.3 "The wide schema tuple is twelve bytes" defines the catalog-resolved wide path. Strict compact object bodies contain `82 F2 F0 82`; their paired inline entity bodies contain `F0 F1` before identity and `F2 F0` after the fixed bridge. These ordered byte equalities do not supply a token-to-name mapping. The associated catalog contains the three wide-path names at different ordinals.

**Need.** We must establish each compact token's selection through an independent schema codebook or a CATIA-authored differential specimen. Name presence and alignment with the wide production do not resolve the compact tokens.

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

### DI-27. Feature parameter membership and order

**Question.** Which incidence selects a feature's direct parameter collection, and which field gives each member's position?

**Known.** `catia.md` §7.3 preserves immediate design-object incidence, structural ancestry, reference-list order, and serialized field positions. These relations do not establish direct parameter membership or collection position. A solver roster can include non-parameter entities and omit a typed scalar in the same structural group. Typed scalar parameters retain their values and native identities at document scope. Their neutral source-order ordinals do not assert feature collection order.

**Need.** We must identify a complete ordered member incidence and validate it against producer collections with direct and recursive membership distinguished. Byte position and partial solver rosters cannot supply that incidence.

### DI-28. Camera, capture, and annotation relations

**Question.** Which native incidences assign annotation-set ownership, capture membership, active TPS-view state, and annotation geometry targets?

**Known.** Cameras, TPS views, annotation sets, and captures have separate relations. An annotation set owns annotation, capture, and TPS-view collections. A capture owns a set reference and visually manages annotations and TPS views. Its current state governs future TPS membership; its active-view and viewpoint states govern display behavior. These states do not define a parametric model variant. Native `CATCameraFeat` catalogs and ordered camera programs retain camera data. View and capture field-class names can pair with tolerance, circle, and model-schema values; the class name alone does not identify an instance or membership relation. Range-only `DiameterThread` and `FeatureRSUR` dimensions are separate from the two-selector constraint-range production. Their retained values do not identify an annotation set, capture, view, or geometric target.

**Need.** We must identify complete native set-owner, ordered collection, capture-membership, active-view, and geometry-target incidences. Validate each relation independently against producer collections and properties. A named camera, shared structural ancestry, numeric equality, or a display state cannot supply a missing relation. Document design configurations require their own model-variant production.

## 3. Standard nested `V5_CFV2`

### SN-02. Rolling-ball scalar-jet semantics and tail control

**Question.** What does the explicit A-family rolling-ball scalar jet parameterize, which incidence identifies its target, and what does the tail control `03` or `05` mean?

**Known.** `catia.md` §6.2 "The rolling-ball tail starts after the knot lane" defines the complete 79-byte, 87-byte, and `87 + 24*K`-byte productions. The scalar jet contains no curve identity. Its sampled values fit class-`15` parameter domains and include exact source knots. Four NIST source curves have overlapping domains, so containment cannot select a target. The absent-jet branch supplies no map. Both raw controls remain separate from neutral orientation.

**Need.** Establish the scalar jet's meaning and target through an independent construction incidence. Establish tail-control semantics through an independently identified field in another encoding or a controlled producer change. Preserve these fields without assigning a target or sign until those relations are proven.

### SN-03. Width-coded class-`0x5e` header token

**Question.** What generic object metadata does the compact header integer of a `b2`, `b3`, or `b4 03 5e` record represent?

**Known.** `catia.md` §6 "Header width and flag are independent" defines the canonical compact integer and its independent retention. The integer does not select an edge payload grammar or supply an object identity. Different values can occupy the same boundary-cycle role. The retained allocation-owner graph does not determine the integer.

**Need.** Establish the metadata meaning through a controlled producer save that measures the exact streamed objects. Do not derive it from partial allocation ownership or assign a link-count meaning from distribution alone.

**Note.** The canonical A5 class-`32` header does not select a rolling-ball variant. Its generic meaning also remains unknown. A controlled producer save that measures the exact streamed object must establish the meaning before a fresh writer can derive the header value.

### SN-04. Class-`0x5e` terminal byte

**Question.** What does the terminal byte of a `b2`, `b3`, or `b4 03 5e` record control?

**Known.** The width-coded terminal is a single-byte `allocation_ref` with the seven admitted value/form pairs `(0,backward_distance)`, `(0,selector2)`, `(8,backward_distance)`, `(8,selector2)`, `(9,backward_distance)`, `(10,backward_distance)`, and `(10,selector2)`, encoded as `01`, `02`, `21`, `22`, `25`, `29`, and `2a`. The object-stream record additionally admits `(9,selector2)`, encoded as `26`. The raw byte and decoded value/form are retained independently of the endpoint identities, and the terminal exhausts the payload after the five references.

**Need.** We must determine what the decoded value means, whether it has any writer-level relation to a successor definition or carrier construction, and whether the four-reference width-coded variant has the same role. Do not use the local successor-layout correlation as a universal endpoint-orientation rule.

**Note.** The four-reference grammar is established separately in `catia.md`
§6.3 "A separate width-coded class-`5e` production". Its four operand roles and
terminal meaning remain unresolved. Exact framing does not supply those roles.

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

### OS-02. Class-`0x5f` terminal controls

**Question.** What is the semantic difference between terminal controls `03` and `05` in a `b5 03 5f` face?

**Known.** Both values terminate the same framed face record.

**Need.** We must know the difference to validate and write the face.

### OS-04. Object-stream body dimension

**Question.** Which source ownership relation distinguishes a closed skin from a volume body?

**Known.** `catia.md` §6.7 "A source-closed face-body ownership chain" defines the complete `65 -> 60 -> 61 -> 5f` payloads. Their counts and terminal controls supply no body-kind enum. Face-edge closure alone does not distinguish a shell attached to a body from a shell bounding a volume.

**Need.** We must know the cell and domain dimensions to preserve source body kind.

### OS-05. Object-stream outward-shell sign

**Question.** Which source field gives the outward-shell sign?

**Known.** `catia.md` §6.7 "**Object-stream topology:**" defines the radial orientation equations used to select a consistent neutral shell gauge.

**Need.** We must know the source sign to preserve native shell orientation.

### OS-06. Object-stream edge terminal controls

**Question.** What is the semantic difference among edge terminal controls `01`, `02`, `21`, `22`, `25`, `26`, `29`, and `2a`?

**Known.** `catia.md` §6.7 "`b5 03 5e` (edge node):" defines the common edge record and its topology incidences. The eight terminals have exact single-byte allocation-reference decompositions. Distinct edge objects with identical five ordered references can use different terminals, as specified in `catia.md` §6.7 "Distinct edge objects can have identical ordered curve".

**Need.** We must determine the terminal's selection rule and the allocation context that selects its value and form. Equal edge geometry and endpoint incidences do not supply that rule. A fresh native writer needs evidence for the terminal; existing records retain their exact byte.

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


## 5. Zero-entity `a9 03`

### ZE-05. Zero-entity ownership-root selection

**Question.** How does a zero-entity stream select its ownership root when more than one face-roster/shell/body triple is structurally valid?

**Known.** `zero_entity_ownership_roots` recognizes every contiguous `[0x6142, 0x6006, 0x6508]` triple with checked fields and retains the candidates in source order. The neutral zero-entity route binds ownership only when exactly one candidate exists. Multiple candidates remain native and do not select a body or shell.

**Need.** We must know the terminal/root identity rule or reject multiple valid ownership triples before transferring body and shell ownership.

## 6. E5 `0D 03`

### E5-05. Higher-topology root signs

**Question.** Which higher-topology relation does `H_root` encode, and how does a root with `H_root=−1` or `S_root=+1` compose orientation?

**Known.** The lane is `H_root, R_face[0..face_count], S_root`. The face-aligned word starts at index one. The rooted production with `H_root=+1, S_root=−1` gives `g_loop=q_loop×F_face×R_face×S_root`; `H_root` does not enter that product.

**Need.** We must identify the higher-topology relation and its other endpoint-sign productions.

### E5-07. Sampled correspondence interpolation

**Question.** What interpolation and derivative rules apply between mode-5 correspondence sites?

**Known.** The complete mode-5 layout, facet order, sample pairing, independent parameter directions, accuracy and fit fields are specified in `catia.md` §9. The samples define corresponding sites. Two sites alone do not prove an affine map between different curve parameterizations.

**Need.** Determine the interpolation rule from the producer's equivalent-parameter implementation or an independent interior-parameter witness.

### E5-11. Active extrapolation continuation

**Question.** What scalar roles and control values define an active E5 NURBS extrapolation branch?

**Known.** `catia.md` §9 "The E5 `0xaa` current-limit program" defines the limit and affine fields. Class `0xaa` has one extrapolation control. The E7 surface program has three flags, eight continuation scalars and six suffix controls. Zero controls disable extrapolation.

**Need.** We must identify an active branch before interpreting its continuation fields.

## 8. Appearance

### AP-01. `SelectingFeatureForColorUuid` presentation target

**Question.** Which presentation object does `SelectingFeatureForColorUuid` select?

**Known.** `catia.md` §7.4 "A `FeatureForColor` schema selection can carry" defines the color, selector atom, and UUID-named entry as distinct source values. Their framing does not assign a B-rep face. Governing FBB rows independently supply effective face colors.

**Need.** An independent presentation-object incidence must establish the selector's target namespace. A face-selector rule must identify a native face independently before binding its appearance.

**Note.** The four reference `FeatureForColor` groups in CTC-05 and STC-10 carry black-alpha-127 assets, selector atom `1`, and UUID-named atom `12`. Their 434 governing FBB rows are all pale blue. The sibling AP242 black styles reach 20 annotation-curve occurrences and 68 tessellated-annotation occurrences, respectively, and zero advanced faces. These groups supply no face-selector witness.
