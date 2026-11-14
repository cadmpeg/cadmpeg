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

### DI-04. Opaque inline values and head-slot namespaces

**Question.** What roles do the values in opaque childless `7C09` bodies have, and which complete productions establish the namespace of the second reference in compact lead-`12` and lead-`52` heads?

**Known.** `catia.md` §7.3 defines the complete inline self, anchor, and optional value incidences separately from ordinary head roles. It also defines opaque childless-body admission through exact paired entity/object cardinality. Opaque retention does not assign roles to the enclosed values. A resolved anchor field's own owner, class, and storage slots remain separate from the inline incidence.

**Need.** An independent schema or behavior check must assign each opaque value's namespace and distinguish head-slot roles before those values can supply typed fields or neutral dependencies.

**Note.** The 15 complete compact inline records in `v6-engine-hex-screw-m8x55.CATPart` select earlier lead-`12` anchors. Each anchor's second head reference equals its paired entity identity and exceeds the associated catalog's 495-entry range. The inline anchor incidence still selects an exact same-graph field. This does not establish a replacement head-role order. Repeated values, adjacent values, and a coincident catalog name do not establish a role for an opaque inline value.

### DI-05. Compact inline schema selectors

**Question.** Which schema entries do the compact inline tokens `F2`, `F0`, `F1`, `69`, and `6A` select?

**Known.** `catia.md` §7.3 "The wide schema tuple is twelve bytes" defines the catalog-resolved wide path. Strict compact object bodies contain the exact tuples `82 F2 F0 82` and `82 69 6A 82`. For the `F2 F0` form, paired inline entity bodies contain `F0 F1` before identity and `F2 F0` after the fixed bridge. These ordered byte equalities do not supply a token-to-name mapping. The associated catalog contains the three wide-path names at different ordinals.

**Need.** We must establish each compact token's selection through an independent schema codebook or a CATIA-authored differential specimen. Name presence and alignment with the wide production do not resolve the compact tokens.

### DI-07. Compound relation-program result binding

**Question.** Does the trailing `d` incidence of a lead-`54` compound relation-program frame select its result when the selected class is `paramout`?

**Known.** `catia.md` §7.3 "A compact compound relation-program instance is" and `catia.md` §7.3 "A separator-form compound relation-program instance is" defines lead-`12` and lead-`54` relation-program frames. Lead-`12` selects `ref(h)` at decoded field 13 as its result when the exact same-graph class is `paramout`. The program identity, repeated reference, context identity, and lead-`54` trailing identity remain distinct incidences. The lead-`54` `d` atom is decoded field 15. A complete typed expression and its source symbols give the ordered inputs.

**Need.** We must establish the lead-`54` result incidence independently of its current class predicate before transferring a separator-form result.

**Note.** The 79-file byte census has nine lead-`12` frames whose `h` selects entity 272, class `paramout`, and three lead-`54` frames whose `d` selects entities 60, 52, and 100, class `Volume`. All other incidences in those frames select other roles. The existing positive lead-`54` fixture is synthetic; the real frames establish its trailing incidence and rejection of `Volume`. A positive result assignment awaits a real witness. The code decides what the specification leaves open: `native.rs:2689` `output_entity` applies the same `paramout` predicate to the lead-`54` trailing slot as to lead-`12` `ref(h)`, and `formula.rs:374` and `decode.rs:1148,1154,1161` consume that slot as a bound formula output and counted result.

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

### DI-24. PMI dimension quantity and suffix framing

**Question.** Which field gives the physical quantity of a `Range`/`CstAttr_Dimension` nominal and its deviations, and what do the `B8`, `C1`, `DC`, and `DF` suffix framings select?

**Known.** `catia.md` §7.3 defines the interval, nominal, constraint, suffix-framing, and owner-incidence productions. `B8` and `C1` retain a constraint evaluation without a Range nominal; `DC`/`81 DB` and `DF`/`81 92` also supply the Range nominal production. The same scalar and deviation program can use either latter framing. Length, angle, and ratio semantics stay unassigned. A source-closed sketch incidence can retain an opaque constraint; the scalar and deviations remain untyped native values. Emission from this production stays native. `attributes.dimension-quantity-unresolved` reports each finite scalar withheld for this reason. Range-only `DiameterThread` and `FeatureRSUR` productions are separate source-closed size dimensions whose definitions establish length semantics. `DI-18` retains the separate feature-local `GSMProject` millimetre-diameter incidence and the remaining Range ownership questions.

**Need.** We must identify the serialized quantity source and its join to the constraint-range entity before transferring the scalar and deviations as length, angle, or ratio values.

### DI-27. Feature parameter membership and order

**Question.** Which incidence selects a feature's direct parameter collection, and which field gives each member's position?

**Known.** `catia.md` §7.3 preserves immediate design-object incidence, structural ancestry, reference-list order, and serialized field positions. These relations do not establish direct parameter membership or collection position. A solver roster can include non-parameter entities and omit a typed scalar in the same structural group. Typed scalar parameters retain their values and native identities at document scope. Their neutral source-order ordinals do not assert feature collection order.

**Need.** We must identify a complete ordered member incidence and validate it against producer collections with direct and recursive membership distinguished. Byte position and partial solver rosters cannot supply that incidence.

### DI-28. Camera, capture, and annotation relations

**Question.** Which native incidences assign annotation-set ownership, capture membership, active TPS-view state, and annotation geometry targets?

**Known.** Cameras, TPS views, annotation sets, and captures have separate relations. An annotation set owns annotation, capture, and TPS-view collections. A capture owns a set reference and visually manages annotations and TPS views. Its current state governs future TPS membership; its active-view and viewpoint states govern display behavior. These states do not define a parametric model variant. Native `CATCameraFeat` catalogs and ordered camera programs retain camera data. View and capture field-class names can pair with tolerance, circle, and model-schema values; the class name alone does not identify an instance or membership relation. Range-only `DiameterThread` and `FeatureRSUR` dimensions are separate from the two-selector constraint-range production. Their retained values do not identify an annotation set, capture, view, or geometric target. `catia.md` §7.3.1 transfers the characteristic, magnitude, datum frame, and secondary activation of a tolerance record, and each `CATMmrTTRS` root as a source shape aspect. In FTC-07 the parallelism `0.762` record selects a chain that reaches `ShortBrpLocation` label `2(E,227)`. The surface-profile `0.762` record selects an aggregate with 32 members; one member reaches label `2(E,220)`, and recursive traversal reaches 33 locations. That record also has `Seg3TolNumValue` and `Seg3TolTabValue` fields with a stored `0.03` mm value and an activation role separate from the `TolWiDRF` declaration.

**Need.** We must identify complete native set-owner, ordered collection, capture-membership, active-view, and geometry-target incidences. Validate each relation independently against producer collections and properties. A named camera, shared structural ancestry, numeric equality, or a display state cannot supply a missing relation. Document design configurations require their own model-variant production. Each `CATMmrTTRS` root needs the face or edge relation that it selects, and the `Seg3` segment needs its tolerance role. A `TolWiDRF` key that zero or several distinct field members carry, or a key in a field with a member that fails to resolve, needs its owner relation: the secondary value transfers with `attributes.pmi-field-key-unresolved`, and the key stays native.

### DI-29. Sheet topology roots

**Question.** Which native records own sheet bodies and their surface representations?

**Known.** FTC-07 has one FBB group with 304 face rows. `SurfacicReps` supplies analytic carriers for 284 of those faces. Its STEP twin contains a 304-face solid and two separate one-face sheets. The Fill Boundary field selects the sketch by both entity reference and persistent key. Six native line programs form two directed triangular components in the sketch chart. The source endpoint references select the six shared point records. The chart lifts the components to the STEP sheet vertices; the largest line-to-point residual is `0.000000942 mm`. The two reconstructed sheet areas are `7.7961625330 mm²` and `7.7961620861 mm²`, versus `7.7961625247 mm²` and `7.7961620847 mm²` in STEP. A seventh line extends beyond the two components and has a different management-flag field. The decoder transfers independent closed components as sheet bodies and reports `topology.sheet-components-inferred` for member exclusion and result grouping.

**Need.** Establish the management-flag and construction-state meanings and the Fill result grouping. Transfer the source-selected chart and directed boundary uses with their identity encoding.

**Conflict.** The current owner-first head interpretation groups records by runtime class. The paired definition suffix identifies that class; the next head operand selects the owner. Compact references and escaped `80` references use different identity encodings. Applying a one-based correction to the escaped form selects the wrong owner or a sketch itself.

### DI-30. Source-schema selector indexing

**Question.** Which source-schema identity and catalog index does each compact or fixed-width selector use, and how does a selected value program bind to its owner?

**Known.** `catia.md` §7.3 defines the current fixed-width selector lookup and the value-chain productions. FTC-07 has compact definition bytes `F3 F4` on both `0.762` scalar candidates. Reading each byte as a one-based identity selects the catalog entries `LENGTH` and `TolNumValue`. A fixed selector `324` on a `0.127` scalar selects `RefTolNumValue` under the same interpretation.

**Need.** Resolve complete selector and value-program framing across definition, value, and suffix fields. Preserve each decoded scalar and its source owner when changing a lookup. Establish contextual entity-reference encoding independently of schema indexing.

**Conflict.** The current lookup selects the catalog entry at the stored fixed-width ordinal. The one-based interpretation selects the preceding entry and gives scalar-compatible roles for these controls. A partial change also alters the number of selectors in a definition and changes downstream value-chain admission.

### DI-31. Non-semantic feature control frame content

**Question.** Which fields of a `CATTPSNonSemanticGDT` annotation and its drafting object graph give the tolerance characteristic, the magnitude with its unit, and the datum references?

**Known.** In FTC-07 the general-note frame `profile .06 | A | B | C` has one `CATTPSNonSemanticGDT` entity and no `CATTPSToleranceWithDRF`, `CATTPSToleranceWithoutDRF`, or `TolBloc` production; the 20 typed tolerances and their 6 secondary values account for every other STEP tolerance. No source field stores the magnitude as binary64 `1.524` or `0.06`, or as binary32 `0.06`; the binary32 `1.524` occurrences lie outside every entity record. The drafting catalog stores the frame as consecutive entries `DftGDT`, `ToleranceType`, `BoxNumber`, `UpperText`, `StringList`, and four RTF cells whose text is `.06`, `A`, `B`, and `C`. No decoded field references the four cell entries. The `StringList` object record has an empty payload, and the records for entities 95–97 have no decoded head. The only unit text is the general note `UNITS: INCHES`.

**Need.** A typed characteristic, a numeric magnitude with a typed unit, and an owner relation from the annotation to its cells transfer the frame as a semantic tolerance. Without them the frame stays native, and the part transfers 26 of the 27 STEP tolerances.

## 3. Standard nested `V5_CFV2`

### SN-02. Rolling-ball scalar-jet semantics and tail control

**Question.** What does the explicit A-family rolling-ball scalar jet parameterize, which incidence identifies its target, and what does the tail control `03` or `05` mean?

**Known.** `catia.md` §6.2 "The rolling-ball tail starts after the knot lane" defines the complete 79-byte, 87-byte, and `87 + 24*K`-byte productions. The scalar jet contains no curve identity. Its sampled values fit class-`15` parameter domains and include exact source knots. Four NIST source curves have overlapping domains, so containment cannot select a target. The absent-jet branch supplies no map. Both raw controls remain separate from neutral orientation.

**Need.** Establish the scalar jet's meaning and target through an independent construction incidence. Establish tail-control semantics through an independently identified field in another encoding or a controlled producer change. Preserve these fields without assigning a target or sign until those relations are proven.

### SN-03. Width-coded class-`0x5e` header token

**Question.** What generic object metadata does the compact header integer of a `b2`, `b3`, or `b4 03 5e` record represent?

**Known.** `catia.md` §6 "Header width and flag are independent" defines the canonical compact integer and its independent retention. The integer does not select an edge payload grammar or supply an object identity. Different values can occupy the same boundary-cycle role. The retained allocation-owner graph does not determine the integer.

**Need.** Establish the metadata meaning through a controlled producer save that measures the exact streamed objects. Do not derive it from partial allocation ownership or assign a link-count meaning from distribution alone.

**Note.** The canonical A5 class-`32` header does not select a rolling-ball variant. Its generic meaning also remains unknown. A controlled producer save that measures the exact streamed object must establish the meaning before a fresh writer can derive the header value. A class-`5b` frame candidate has a four-byte compact header and a count-one reference to the following class-`61` roster. Its suffix reference interpretations select unrelated record classes across peer frames; suffix semantics and allocation continuity remain unresolved.

### SN-04. Class-`0x5e` terminal byte

**Question.** What does the terminal byte of a `b2`, `b3`, or `b4 03 5e` record control?

**Known.** The width-coded terminal is a single-byte `allocation_ref` with the seven admitted value/form pairs `(0,backward_distance)`, `(0,selector2)`, `(8,backward_distance)`, `(8,selector2)`, `(9,backward_distance)`, `(10,backward_distance)`, and `(10,selector2)`, encoded as `01`, `02`, `21`, `22`, `25`, `29`, and `2a`. The object-stream record additionally admits `(9,selector2)`, encoded as `26`. The raw byte and decoded value/form are retained independently of the endpoint identities. The terminal exhausts the payload after either four or five complete references. The separate four-reference native record retains its complete source frame without assigning operand roles or neutral topology.

**Need.** We must determine what the decoded value means, whether it has any writer-level relation to a successor definition or carrier construction, and whether the four-reference width-coded variant has the same role. Do not use the local successor-layout correlation as a universal endpoint-orientation rule.

**Note.** The four-reference grammar is established separately in `catia.md`
§6.3 "A separate width-coded class-`5e` production". Its four operand roles and
terminal meaning remain unresolved. Exact framing does not supply those roles.

### SN-05. Implicit-axis class-`0x18` line direction

**Question.** Which signed unit direction do controls `48` and `88` imply, and what are the fields of the separate control-`80` descriptor grammar?

**Known.** `catia.md` §6.4 defines the finite scalar counts, constant-axis tuples, zero elision, explicit unit direction, and owner-chart midpoint and half-span relations. The implicit controls identify the varying axis. A control-`80` descriptor can precede class `25` and carries a different four-scalar lane.

**Need.** Bind implicit-axis parameters to directed endpoints or derivatives to determine the sign. Bind the control-`80` descriptor to its following definition before assigning the four scalar roles.

### SN-06. Reversed class-`0x23` parameter slots

**Question.** Which directed parameter mapping applies to reversed `(2,1)` slots in a non-identity class-`0x23` map?

**Known.** `catia.md` §6.3 defines the eight-scalar affine map, local slots, fixed operand, and adjacent circle binding. The reversed identity form has identical source and target intervals, unit scale, and zero offset; either direction gives the same map.

**Need.** Bind both parameter representations of a reversed non-identity map independently to determine its direction.

### SN-07. Class-`0x24` operand namespaces

**Question.** Which source-local parameter representations do scalar class-`0x24` operands reference?

**Known.** `catia.md` §6.3 "A scalar class-`24` domain relation begins" defines the complete selector grammar, source relation, target sites, and signed accuracy closure. Compact class-`24` operands close the two adjacent edge-use vectors. Scalar operands retain their source index and logical offset without a carrier binding.

**Need.** Resolve scalar operands through an independently bound source-local owner before applying the parameter relation to a carrier. Obtain a complete witness before admitting any selector outside the listed grammar, including `0986`.

### SN-39. Scalar parameter-map fit-tolerance units

**Question.** Does the scalar class-`25` fit tolerance bound Cartesian displacement, a representation parameter, or a scaled error measure?

**Known.** `catia.md` §6.3 "Every scalar is finite. Lead `0a` appends" defines a finite nonnegative fit-tolerance field distinct from signed accuracy. The operands select parameter representations. For corroborated planar shared edges the decoder interprets that tolerance as a model-space bound in mm and reports `geometry.parameter-map-accuracy-inferred`.

**Need.** Author controlled changes to geometric scale and parameter scale independently. Determine which scale changes the tolerance before assigning it a physical unit in the format model.

**Conflict.** The decoder's planar repair uses a model-space interpretation while the format model leaves the field's unit unresolved.


### SN-08. Class-`0x25` remaining allocation forms

**Question.** Which allocation identities do the remaining scalar class-`0x25` operand forms select, and what do the separate multi-operand productions encode?

**Known.** `catia.md` §6.3 "A B-family scalar class-`25` parameter map" defines the complete affine and sampled maps, allocation addressing forms, sites, fit tolerance, and adjacent class-`06` use-pair framing. The same section defines the target and source representation order and the width-coded allocation addresses. B-family class `25` and A8 class `25` have separate headers and payloads. Two-operand scalar maps retain exact sample pairing and source positions. The separate A8 sampled parameter map is defined in §6.7. An A-family scalar sampled map can carry more than twelve site pairs. Its operands select the same representations as its class-`62` owner occurrences.

**Need.** Transfer the wider A-family sampled map and its selected representations. Establish the address semantics for the remaining operand forms through independent owner and carrier relations. Resolve the multi-operand payloads beginning `83` or `84` separately. Obtain a complete witness before admitting an unlisted scalar selector.

**Conflict.** A record-distance reading and a weighted-address reading can select different wrappers for a backward-distance operand. Their parameter ranges do not distinguish the alternatives when the selected wrappers have no evaluable domain.

In FTC-07, treating consecutive operand values as one parameter representation joins source operands `7687` and `7688`, whose stored source intervals are approximately `[-58.658, 58.658]` and `[0, 0.901]`. The adjacent use-pair framing supplies no parameter transform between those operands. Two candidate map-to-side matches have matching source intervals and adjacent face carriers. Their chart-side operands are `2` and `6`, and their respective map source operands are `7650` and `7659`. A single additive offset would require both `7648` and `7653` to hold both matches. The 80 chart-side frames and 439 distinct source-scoped records selected by resolved edge endpoint references form disjoint sets.

### SN-40. Overlapping three-reference endpoint station tags

**Question.** Which layout applies to an 18-byte lead-`80` incidence tail when both the first scalar's tag position and the final tag position hold a compact code?

**Known.** `catia.md` §6.4 "The following incidence tails preserve separate parameter gauges" defines the first-tag and final-tag layouts when each has a unique tag position. Both guards match the tail `80 <0.5:f64le> 05 <f64le bits 3d00000000000000>`. The decoder keeps a valid first-tag reading, otherwise takes a valid final-tag reading, and reports `topology.endpoint-station-reading-inferred` with the selected reading and parameter.

**Need.** Obtain a real-file witness with an independently established endpoint station that distinguishes the two valid readings. Determine whether an owner relation or an additional selector identifies the layout.


### SN-09. A8 parameter-map interpolation and additional operand productions

**Question.** Which interpolation rule does `81 15` select between stored parameter pairs, and what do A8 class-`25` payloads beginning `84` encode?

**Known.** `catia.md` §6.7 defines the complete `82` two-operand sampled map, positional count, separate paired arrays, either co-curve orientation, nonnegative fit tolerance, active reference interval, and direct or class-`24` wrapper operands. A curve can be evaluated through its first pcurve and support without interpolating the map. The pair table contains no Cartesian poles.

**Need.** Resolve the interpolation selector before mapping arbitrary stations into the co-curve gauge. Parse the `84` operand production independently before admitting it as a two-operand map.

### SN-12. Serialized geometry-factory context and remaining support relations

**Question.** Which serialized relation identifies a consolidated pcurve's geometry factory and binds its remaining persistent support tags to identity-bearing records?

**Known.** `catia.md` §6.3 "When the `op1` support identity equals" defines the exact unique class-`0x19` binding, the unique cylinder compact-header operand binding, and the unique standard `00 33` carrier binding. Before the standard-carrier lookup, §7.5 outer alias rows canonicalize a raw `op1` tag only when exactly one retained alias row supplies one canonical surface-roster tag. Direct surface-support rows preserve their tag; grouped non-surface rows require one direct row in the same exact group. Duplicate raw tags, duplicate group targets, missing group targets, unknown geometry, and failed endpoint lifts remain unresolved. B5 object-stream references and allocation operands remain separate namespaces.

`catia.md` §6.3 "The compact tag is separate" defines factory-local tag scope and the separate addressing forms. Complete consolidated analytic and freeform carrier payloads without their own persistent surface tag supply geometry through independent construction or geometric witnesses.

**Need.** Identify the serialized factory context and any additional typed identity relation for support tags outside the admitted circle, cylinder-header, standard-carrier, and canonical-alias relations. Require a source-closed relation and its chart and endpoint invariants.

**Conflict.** A unique class-`19` record with tag 125 and a pcurve with `op1=125` occupy the same source extent, but their U intervals differ and the pcurve is not constant in V. Distinct tags 113 and 126 lift to the same five-site geometric locus through different cylinder charts. Same-source containment, numeric uniqueness, and geometric coincidence do not identify the owning factory or object.

**Note.** In FTC-07, class-`5f` and class-`62` owner references bind two class-`20` jets to consecutive spherical edge uses. Each jet interval equals its class-`25` source interval. The class-`06` compressed station controls need an independent decoding.

### SN-15. Additional standard-circle branch relations

**Question.** Which native incidence or chart relation selects a standard circle's edge interval when the admitted witnesses in `catia.md` §5.8 leave the branch unresolved?

**Known.** `catia.md` §5.8 "Cylinder and torus payloads use" identifies their witness point as the trimmed-face AABB center and defines a branch when one candidate's exact extrema fit all available incident-face boxes. The same bounds block exists on plane, cone, and sphere records. A box center is a spatial bound, independent of an edge's angular interval.

**Need.** Establish a source-bound parameter or boundary incidence for the remaining branches. Require independent interior-arc verification and agreement between incident carriers before extending the witness relation.

**Conflict.** In FTC-07, a cylinder face's AABB-center azimuth lies about `1.38e-7` radians beyond an edge endpoint. Strict angular containment selects the complementary arc, whose extrema exceed the face box by more than 3 mm. The STEP twin's short arc and the native face box select the other interval.

### SN-17. Unbound fixed-nine carrier context

**Question.** Which source-closed relation supplies carrier context for a fixed-nine parameter rectangle when the seven-record owner chart is absent?

**Known.** `catia.md` §6.4 "A fixed-nine numeric tail stores" defines the inline parameter rectangle and its separate carrier context. Tagged alternating and width-coded numeric tails also store a conservative Cartesian envelope of the limited surface section. The local backward-reference walk resolves topology allocations; it does not select an identity-free carrier. The admitted seven-record chart provides an exact carrier relation, independently of the reference dialect.

**Need.** Identify another source-closed carrier relation before evaluating an unbound rectangle as owned geometry. Preserve the parameter and Cartesian lanes independently of whether that relation resolves.

**Conflict.** All 112 tagged packets admit at least one A5 geometry candidate under domain, corner, and 25-by-25 interior checks; 40 admit multiple candidates. Candidate uniqueness supplies no serialized identity. In the spring witness, the alternative carrier at offset 52715 passes the corner-box test but fails at interior sites. Its U domain also ends 0.0010119222600337707 parameter units below the stored rectangle; the interior comparison uses the common domain. Neither numeric proximity nor envelope containment assigns a carrier.

**Note.** The all-compact face-boundary predicate remains separate. Tagged alternating and width-coded envelopes do not bind a standard face by its AABB or by source position.

### SN-18. Eight-reference construction identities

**Question.** Which objects do the last three references of an eight-reference class-`37` construction select, and which source relation proves their geometric roles?

**Known.** `catia.md` §6.4 "An eight-reference B-family" defines the complete reference program, both control tuples, zero lane, terminal, and source-local rolling-ball carrier relation. Ordinary local owner/node joins use the checked successor relation; closed four-edge preludes use the separate ninth-identity-plus-ten relation. Neither local relation supplies a standard face ordinal. Standard face order belongs to the independent surface roster, whose complete rows contain no owner or node identity field.

**Need.** Resolve the referenced objects in their actual allocation or factory namespace and verify their geometry against the rolling-ball center and limiting loci before assigning those roles. Preserve every unassigned reference and addressing form.

**Conflict.** The public generic-fillet description lists center points, first limit, and second limit. The serialized jet stores first limit, second limit, and center instead. Public constructor descriptions therefore do not establish bridge wire order. In the spray-bottle hand record, references 781, 778, and 780 have no retained outer alias-row target; the `[base+3,base,base+2]` numeric pattern alone supplies no locus identity.

**Note.** Independent standard-versus-STEP topology verifies all 134 eyebolt faces uniquely by joint adjacency refinement. The spring graph has 42 indistinguishable face pairs and admits an exact graph isomorphism without a unique face assignment. Those geometric/topological associations do not turn a local owner integer into a standard ordinal.

### SN-23. Class-`0x61` roster controls and member identities

**Question.** Which owning contexts select roster controls `3` and `8`, and which native relation selects the current result from multiple valid body roots?

**Known.** `catia.md` §6.4 "Member bits retain serialized occurrence order" defines the reference forms, packed control, occurrence bits, and closure. Source-closed class-`65`/`60`/`61` chains with a one-child class-`60` owner use control `1`. A one-child allocation list does not by itself identify the owning domain. The wide control-`1` roster binds its members through the allocation widths in the frame headers. Control `3` identifies the sheet-group roster form in §6.4, with member orientation established in the native rolling-ball chart. Other carrier charts use inferred parity. The owning domain of control `8` remains open.

**Need.** Resolve the active-result relation from the feature or container data. Verify chart parity for additional carrier forms. Establish a selector oracle for control `8`.

**Conflict.** A source-local 53-member roster and the current 700-face shell have different cardinalities. Descending numeric reference order agrees with six independent face-orientation sequences, but numeric order alone does not supply an allocation-to-standard identity relation.

### SN-25. Local boundary-domain location and final identity

**Question.** Which controls select the location of a local boundary domain, and which source-closed relation attaches it to final topology?

**Known.** `catia.md` §6.4 "**Adjacent `5f,62` relation:**" defines the local face-to-boundary-domain incidence, paired counted lane, fixed-nine dialect, scoped adjacency, and occurrence identity. A local domain may be open or repeated. The class-`5f` terminal and counted tail do not assign outer, inner, or immersed location. Carrier-chart and simple-cycle evidence are independent refinements.

**Need.** Resolve location and complete member identities before forming a neutral loop. Establish a separate unique geometry or topology relation before attaching that loop to a final face. Preserve unresolved and repeated local domains.

### SN-26. Revolution profile reference namespace

**Question.** Which namespace does the nonzero `u16le` profile reference in a consolidated class-`2d` record address?

**Known.** `catia.md` §5.15 "The `00 33 30` kind tag selects" defines the complete frame, two-byte reference forms, axis chart, and exact unique profile-interval binding. Equality with a circle record identity does not resolve an ambiguous interval. The reference does not establish a revolution object identity.

**Need.** Define a source-closed reference namespace and directrix relation for profiles that lack a unique interval match.

### SN-37. Owner boundary allocation and control meanings

**Question.** Which allocation-scoped relation maps a closed owner boundary to the standard face and edge-support records?

**Known.** `catia.md` §6.4 "A fixed-nine packet has a closed owner-boundary cycle" defines the typed cycle and its endpoint closure. The fixed-nine and counted-owner grammars preserve distinct reference forms and tails. The counted numeric-tail and adjacent A5 chart production constrain a standard face carrier through its stored bounds and vertices. The class-`5b` interval prefix and class-`5c` parallel reference/item lane have bounded structure; neither assigns a standard support identity. A 79-file source sweep contains 94 contiguous ten-record boundary blocks, of which 86 close under the five-reference edge grammar. The other eight use the separate four-reference edge production. A broader nine-allocation-reference sweep contains 135 closed four-edge graphs: 131 have five-byte tails and four have 62-byte tails. The fixed-nine native route retains two of the four long-tail graphs; the other two use the counted allocation-reference grammar. These populations have different admission rules.

**Need.** Establish a source-closed allocation bridge to standard support identities. Assign the class-`5b` interval frame and suffix, class-`5c` item and terminal meanings, and the remaining owner-tail controls from an independent semantic relation. Broader counted-packet cycle projection requires its own complete endpoint and source-closure rules.

**Conflict.** The proposed universal face-node offset `target − final_owner_reference = m + 1` for a second owned-child operand `m ≥ 2` has three source counterexamples: spray-bottle at `369117` gives `m=2`, difference `0`; NIST CTC 01 at `1887263` and `1920192` gives `m=6`, difference `−7`. The bounded `27 03`/`27 05` prelude with difference `+10` remains a separate admitted relation. Numerical overlap across source namespaces does not establish an identity join.

**Note.** Existing native retention passes full model checking for 5,214 copied control frames and 19,277 copied owner frames. Both existing typed boundary cycles match independently reconstructed source endpoints. Unassigned meanings remain open; no cross-source geometry binding follows from these structural results.

### SN-38. Two-edge sheet face orientation

**Question.** Which native relation assigns the absolute face sense of a closed two-edge sheet relative to its surface chart?

**Known.** The class-`62` complement relation determines the endpoint pairing in four isolated faces. The curved face has forward sense in CATIA STEP; two selector-`d8` planar faces have reversed sense. Both planar face boundaries and areas agree with native pcurve lifts independently of their sense. The last singleton class-`61` record before the boundary owner has occurrence bit one on the curved face and bit zero on both selector-`d8` planar faces; the independent tilted planar face has bit one.

**Need.** Prove the last singleton class-`61` to selected-face identity beyond an isolated one-face source and establish its chart-parity rule for additional surface carriers. A third tilted planar face uses carrier and pcurve productions outside the currently decoded variants.

### SN-41. Trimmed-face box extent

**Question.** Which construction produces the stored trimmed-face AABB of a curved face, and what bounds its distance from the exact face extent?

**Known.** `catia.md` §5.8 "The same arc test applies" selects an arc by the smaller excess over the face boxes when neither arc fits the `2e-3` mm allowance. Across 8,601 measured standard faces in 79 files, the box exceeds the exact face extent by at most binary32 rounding on all but 13 faces, and lies inside it on 1,854 faces by more than `2e-3` mm. The following readings are falsified:

- The box is the exact trimmed extent: 1,273 of 4,027 cylinder faces fall short by more than `2e-3` mm.
- The box covers only the boundary vertices: half-cylinder faces whose four vertices lie in one axial plane reach `11.1091` mm from that plane at radius `11.1125` mm.
- The box is a control-net or conservative bound: it never exceeds the exact extent.
- The shortfall is binary32 rounding: `3.4e-3` mm is several hundred ulps at the coordinate scale.
- The shortfall follows a fixed angular step: in one part, radius `2.197` mm falls short by `7.3e-3` mm and radius `11.1125` mm by `3.4e-3` mm.
- The shortfall is bounded by `0.01` mm: 41 files peak at `0.0034`–`0.0100` mm, but 13 files reach `0.020`–`0.26` mm.
- The bound scales with part size: in the first group the peak stays at or below `0.0100` mm for part sizes from `4.8` to `780` mm.
- The saving release selects the bound: files saved by the same release fall in both groups.

The two groups match a tessellation chord tolerance that differs between files. The trim mesh stores handles without coordinates, so a mesh-vertex box has no direct check.

**Need.** A stored or derived per-file tolerance replaces the excess-ratio rule with a containment test, and removes the loss that the ratio rule records.

### SN-42. Boundary gauge without planar evidence

**Question.** Which source relation fixes the absolute gauge of a boundary parity component that contains no evaluable plane-face boundary?

**Known.** `catia.md` §5.4.2 "The face orientation fixes the absolute gauge" anchors a component from the boundaries of its plane faces. Across 79 files, 976 of 1,281 components take an anchor, none receives conflicting votes, and 479 reverse their solved gauge. The following readings are falsified:

- One reversal for all boundaries of a face: incidence-cycle reconstruction starts each cycle at its lowest edge in stored direction, and tying the cycles of one face makes the parity system inconsistent.
- The stored cycle direction is absolute: in FTC-07 it disagrees with the STEP loop direction on faces of every carrier kind.
- The solved gauge of the first boundary is absolute: 479 of 976 anchored components reverse it.
- Every unanchored component lacks a plane face: 39 of the 305 unanchored components contain one.
- The vertex-polygon outer/inner classification anchors plane faces with unevaluable loops: it adds 2 of the 39.
- A curved-face winding anchor reaches the remainder: 273 of the 305 contain no boundary whose every edge has a carrier and an interval.

A component without an anchor keeps the gauge of the parity solve's first boundary.

**Need.** Components bounded only by curved faces, or by plane faces with unresolved edge intervals, have no absolute loop direction; region and volume measurement on spheres and tori depend on it. Resolving their edge carriers and intervals is a precondition for any geometric anchor.

## 4. Object stream

### OS-02. Class-`0x5f` terminal controls

**Question.** What is the semantic difference between terminal controls `03` and `05` in a `b5 03 5f` face?

**Known.** `catia.md` §6.7 "`b5 03 5f` (per-face node)" defines both terminal forms and retains them independently of loop location and face orientation. A 79-file source sweep contains 29,555 counted faces. Among 3,250 outer loops with complete line-only pcurves, 618 contradict a direct terminal-to-winding assignment; the exterior loop control agrees with all measured windings. The terminal value also differs from a referenced loop's primary control in 4,688 resolved loop occurrences. These are different control fields.

**Need.** Establish the terminal's writer-level meaning and selection rule through an independent source relation. Shared byte encodings alone do not assign allocation-addressing semantics.

**Conflict.** The retained private air-filter quote at physical offset `571941` names face `45375` and loop `45372`; the source frame names face `45379` and loop `45376`. The terminal is `05` in both descriptions. The source bytes govern the identity witness.

### OS-04. Object-stream body dimension

**Question.** Which source ownership relation distinguishes a closed skin from a volume body?

**Known.** `catia.md` §6.7 "A source-closed face-body ownership chain" defines the complete `65 -> 60 -> 61 -> 5f` payloads. Their counts and terminal controls supply no body-kind enum. Face-edge closure alone does not distinguish a shell attached to a body from a shell bounding a volume.

**Need.** We must know the cell and domain dimensions to preserve source body kind.

### OS-05. Object-stream outward-shell sign

**Question.** Which source ownership and chart relations assign absolute
face-to-carrier orientation, and how do loop controls and signed shell members
participate in that relation?

**Known.** `catia.md` §6.7 defines the separate loop-location controls,
source-closed ownership payloads, and the bounded selected-plane signed-member
and chart composition. A loop's first control alone does not establish
outward face sense. Native outer-cylinder and negative-X-cap records supply
counterexamples to a universal primary-control rule. A raw loop control,
chart handedness, face occurrence, and outward shell orientation are distinct
relations. The selected standard-plane rule does not establish a universal
member-sign rule for other carriers or owners.

**Need.** Establish the complete selected owner, occurrence, and chart
composition for each supported domain. Preserve the bounded standard-plane
mapping. Determine an independent absolute orientation before transferring
B5 face sense; do not invert all primary controls or replace them with an
unqualified signed-member rule.

**Conflict.** B5 `native_carrier_face_sense` still assigns neutral face sense
from the unique outer loop's first control across its admitted carriers,
with an additional cone chart-parity factor. That positive rule lacks a
complete outward-orientation contract. Standard-family topology can use a
different, independently constrained orientation route.

### OS-06. Object-stream edge terminal controls

**Question.** What is the semantic difference among edge terminal controls `01`, `02`, `21`, `22`, `25`, `26`, `29`, and `2a`?

**Known.** `catia.md` §6.7 "`b5 03 5e` (edge node):" defines the common edge record and its topology incidences. The eight terminals have exact single-byte allocation-reference decompositions. Distinct edge objects with identical five ordered references can use different terminals, as specified in `catia.md` §6.7 "Distinct edge objects can have identical ordered curve".

**Need.** We must determine the terminal's selection rule and the allocation context that selects its value and form. Equal edge geometry and endpoint incidences do not supply that rule. A fresh native writer needs evidence for the terminal; existing records retain their exact byte.

### OS-16. Binary64 values in inline entity bodies

**Question.** Which inline entity-body bytes are `E6 <bits:u64le>` binary64 fields, so that an `EA` among their value bytes is not an identity delimiter?

**Known.** `catia.md` §7.3 "An object graph is preceded by" defines the entity-table run, its `DE` separator, and the unique strictly increasing identity path. Inside an inline body, only `32` five-byte atoms and `FE F6` fixed frames hide `EA` bytes from the path. A complete record run can precede `DE`, a `7C08` graph with the same record count, a `7C0B` value block, and its `7C02` catalog in the declared `CATPrtCont` content stream, while its final inline record has two `EA` candidates above the previous identity: the identity at body offset 7 and an `EA` that is the eighth byte of a nine-byte `E6` field. The path is then not unique, so the graph is not detected and the part has no modeling scope. When every inline `E6` begins a nine-byte field, other complete graph runs lose identities, because some inline bodies have an `E6` byte that is not a binary64 tag before their identity.

**Need.** We must know the inline-body production that starts a binary64 field. Without it, the identity path stays ambiguous and the graph remains outside the modeling scope.

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

## 7. Container and route selection

### CN-01. CATProduct, CATShape, and CGR decode path

**Question.** Which detection and variant productions admit `.CATProduct`, `.CATShape`, and `.cgr` documents, and which production transfers their assembly or tessellated structure?

**Known.** `container.rs:1090` `looks_like_catia` admits any `V5_CFV2` prefix regardless of the document kind, and `lib.rs:112` scores that admission `High`. `variant.rs` names only `CATPart` storage families; any other layout scores `Unknown`. `container.rs:370` retains `.catpart`, `.catproduct`, `.catshape`, and `.cgr` targets only as external-document reference strings with their containing segment identity. `docs/format-support.md` records CATIA product structure as `None`.

**Need.** A CATProduct, CATShape, or CGR specimen and an explicit admission decision must establish whether those kinds decode, which variant each selects, and whether occurrences, placements, or tessellation transfer. A shared container magic leaves product structure undecoded.

### CN-02. FBB-only and E5 coexistence route golden

**Question.** Which frozen fixture pins route selection when a partial FBB-only spine coexists with a coherent E5 walk?

**Known.** `catia.md` §1 and `container.rs:1696` (`identify_variant`) state the precedence: a coherent E5 walk supersedes a partial FBB-only spine but not a complete standard spine. Every dialect-registry witness fixture selects one variant at a time: `fbb_only_fallthrough.catpart` witnesses `catia:fbb-only` and `e5_circle.catpart` witnesses `catia:e5-stream`. A fixture with both a partial spine and a coherent walk is still missing.

**Need.** A frozen coexistence fixture must pin the E5 route selection and its golden snapshots before the precedence rule counts as covered. The documented precedence and the single-variant witnesses leave the coexistence branch uncovered.

## 8. Appearance

### AP-01. `SelectingFeatureForColorUuid` presentation target

**Question.** Which presentation object does `SelectingFeatureForColorUuid` select?

**Known.** `catia.md` §7.4 "A `FeatureForColor` schema selection can carry" defines the color, selector atom, and UUID-named entry as distinct source values. Their framing does not assign a B-rep face. Governing FBB rows independently supply effective face colors.

**Need.** An independent presentation-object incidence must establish the selector's target namespace. A face-selector rule must identify a native face independently before binding its appearance.

**Note.** The four reference `FeatureForColor` groups in CTC-05 and STC-10 carry black-alpha-127 assets, selector atom `1`, and UUID-named atom `12`. Their 434 governing FBB rows are all pale blue. The sibling AP242 black styles reach 20 annotation-curve occurrences and 68 tessellated-annotation occurrences, respectively, and zero advanced faces. These groups supply no face-selector witness.
