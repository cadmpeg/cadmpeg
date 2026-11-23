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

**Note.** The exact legacy catalog opener is known only from constructed fixtures. A plain `CATCatalogManager` string does not establish that production. Constructed `Foo` fixtures test opaque retention and identifier detection only.

### DI-02. Unresolved schema-role selectors

**Question.** What field role does each unresolved schema-selector byte select?

**Known.** `catia.md` §7.1 "Within an identity interval, `<inclusive-length:u8> <name:utf8> <selector>`" defines the byte production and its target selector. A literal role name assigns its role directly. An unresolved selector byte does not.

**Need.** A real legacy specimen and a schema-local role mapping must identify the selected role before assigning field semantics.

**Note.** The required legacy catalog opener, and with it the legacy role-selector production, is known only from constructed fixtures. The selector bytes in constructed framing tests remain unnamed; their following field codes do not supply a role mapping.

### DI-04. Opaque inline values and head-slot namespaces

**Question.** What roles do the values in opaque childless `7C09` bodies have, and which complete productions establish the namespace of the second reference in compact lead-`12` and lead-`52` heads?

**Known.** `catia.md` §7.3 defines the complete inline self, anchor, and optional value incidences separately from ordinary head roles. It also defines opaque childless-body admission through exact paired entity/object cardinality. Opaque retention does not assign roles to the enclosed values. A resolved anchor field's own owner, class, and storage slots remain separate from the inline incidence.

**Need.** An independent schema or behavior check must assign each opaque value's namespace and distinguish head-slot roles before those values can supply typed fields or neutral dependencies.

**Note.** Complete compact inline records can select earlier lead-`12` anchors. Each such anchor's second head reference equals its paired entity identity and exceeds the entry range of the associated catalog. The inline anchor incidence still selects an exact same-graph field. This does not establish a replacement head-role order. Repeated values, adjacent values, and a coincident catalog name do not establish a role for an opaque inline value.

### DI-05. Compact inline schema selectors

**Question.** Which schema entries do the compact inline tokens `F2`, `F0`, `F1`, `69`, and `6A` select?

**Known.** `catia.md` §7.3 "The wide schema tuple is twelve bytes" defines the catalog-resolved wide path. Strict compact object bodies contain the exact tuples `82 F2 F0 82` and `82 69 6A 82`. For the `F2 F0` form, paired inline entity bodies contain `F0 F1` before identity and `F2 F0` after the fixed bridge. These ordered byte equalities do not supply a token-to-name mapping. The associated catalog contains the three wide-path names at different ordinals.

**Need.** We must establish each compact token's selection through an independent schema codebook or a CATIA-authored differential specimen. Name presence and alignment with the wide production do not resolve the compact tokens.

### DI-07. Compound relation-program result binding

**Question.** Does the trailing `d` incidence of a lead-`54` compound relation-program frame select its result when the selected class is `paramout`?

**Known.** `catia.md` §7.3 "A compact compound relation-program instance is" and `catia.md` §7.3 "A separator-form compound relation-program instance is" defines lead-`12` and lead-`54` relation-program frames. Lead-`12` selects `ref(h)` at decoded field 13 as its result when the exact same-graph class is `paramout`. The program identity, repeated reference, context identity, and lead-`54` trailing identity remain distinct incidences. The lead-`54` `d` atom is decoded field 15. A complete typed expression and its source symbols give the ordered inputs.

**Need.** We must establish the lead-`54` result incidence independently of its current class predicate before transferring a separator-form result.

**Note.** Stored lead-`12` frames have an `h` that selects an entity of class `paramout`. Stored lead-`54` frames have a `d` that selects an entity of class `Volume`. All other incidences in those frames select other roles. The existing positive lead-`54` fixture is synthetic; the stored lead-`54` frames establish its trailing incidence and rejection of `Volume`. A positive result assignment awaits a real witness. The code decides what the specification leaves open: `native.rs:2689` `output_entity` applies the same `paramout` predicate to the lead-`54` trailing slot as to lead-`12` `ref(h)`, and `formula.rs:374` and `decode.rs:1148,1154,1161` consume that slot as a bound formula output and counted result.

### DI-08. Legacy relation `body` selector

**Question.** What identity space does a nonlocal `body` selector on a legacy typed relation use?

**Known.** `catia.md` §7.1 "A typed relation consists of" defines the local identity case. A `body` selector equal to the containing identity supplies the local relation context.

**Need.** We must resolve a nonlocal selector to bind the relation context.

**Note.** The required legacy catalog opener is known only from constructed fixtures. The local self-identity rule and synthetic nonlocal rejection do not establish the nonlocal selector namespace.

### DI-09. Legacy relation `param` selector

**Question.** What identity space does a nonlocal `param` selector on a legacy typed relation use?

**Known.** `catia.md` §7.1 "A typed relation consists of" defines the local identity case. A `param` selector inside the containing run selects the relation parameter only when the `body` selector equals the relation's containing identity.

**Need.** We must resolve a nonlocal selector to bind the relation result.

**Note.** The required legacy catalog opener is known only from constructed fixtures. A synthetic same-run binding or nonlocal rejection does not establish an external parameter identity.

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

**Known.** `catia.md` §7.3 "A catalog-resolved value selection named `Range`" defines the complete interval independently of selector cardinality and suffix framing and retains every incoming payload reference and storage selector. Range intervals occur with no slots or with finite and `E8` zero lower and upper tolerance-deviation slots. They occur in `Range`-only, `Range`/`CstAttr_Dimension`, and larger selector sequences. Exact `D8`/`81 93`, `D8`/`81 DB`, and `DC`/`81 DB` scalar suffix dialects supply the nominal independently of selector cardinality. A `Range`-only interval can carry both deviations and its nominal. Range intervals can occupy a stable `MechanicalPart` aggregate slot selected by a `_SpecList`/`FeatureFSUR` relation whose other operand carries `UserPattern` and `SimpleLimit`; sketch-versus-PMI is not an exhaustive ownership choice. Their paired and incoming classes also include presentation, TPS capture-link, limiting-element, Boolean, geometry, and catalog-manager classes. Some intervals have no incoming incidence, one incidence, duplicate references from one object, or references from multiple objects. The narrower two-selector constraint-range production retains five exact selector/code/trailer tuples. `ListAggregator` references can include unrelated and repeated identities. A shared structural design-object owner can contain feature, geometry, and presentation classes. These incidences do not distinguish sketch, feature/model-tolerance, and product-manufacturing-information ownership. A two-selector range transfers as one opaque sketch constraint only when exactly one total incoming incidence resolves to its same-graph paired source entity and object record, and that source object's complete owner chain reaches one transferred `Sketch` before another transferred feature. The source object record is retained as one unresolved native operand. Its nominal, deviations, evaluation, and suffix framing remain native because this production does not assign a physical quantity. Other two-selector ranges remain unresolved. The `T` definition with ordered `ProfileList`/`GSMProject`/`Range` selectors has the feature-local diameter frame and cylinder-face incidence defined in §7.3.

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

**Known.** Cameras, TPS views, annotation sets, and captures have separate relations. An annotation set owns annotation, capture, and TPS-view collections. A capture owns a set reference and visually manages annotations and TPS views. Its current state governs future TPS membership; its active-view and viewpoint states govern display behavior. These states do not define a parametric model variant. Native `CATCameraFeat` catalogs and ordered camera programs retain camera data. View and capture field-class names can pair with tolerance, circle, and model-schema values; the class name alone does not identify an instance or membership relation. Range-only `DiameterThread` and `FeatureRSUR` dimensions are separate from the two-selector constraint-range production. Their retained values do not identify an annotation set, capture, view, or geometric target. `catia.md` §7.3.1 transfers the characteristic, magnitude, datum frame, and secondary activation of a tolerance record, and each `CATMmrTTRS` root as a source shape aspect. A parallelism tolerance record can select a chain that reaches one `ShortBrpLocation` label of the form `2(E,<n>)`. A surface-profile tolerance record can select an aggregate with 32 members; one member reaches a `2(E,<n>)` label, and recursive traversal reaches 33 locations. That surface-profile record can also have `Seg3TolNumValue` and `Seg3TolTabValue` fields that store a millimetre value, for example `0.03`, and an activation role separate from the `TolWiDRF` declaration.

**Need.** We must identify complete native set-owner, ordered collection, capture-membership, active-view, and geometry-target incidences. Validate each relation independently against producer collections and properties. A named camera, shared structural ancestry, numeric equality, or a display state cannot supply a missing relation. Document design configurations require their own model-variant production. Each `CATMmrTTRS` root needs the face or edge relation that it selects, and the `Seg3` segment needs its tolerance role. A `TolWiDRF` key that zero or several distinct field members carry, or a key in a field with a member that fails to resolve, needs its owner relation: the secondary value transfers with `attributes.pmi-field-key-unresolved`, and the key stays native.

### DI-29. Sheet topology roots

**Question.** Which native records own sheet bodies and their surface representations?

**Known.** A part can store its solid in one FBB group and one-face sheets outside it: the FBB face rows cover the faces of the solid, and `SurfacicReps` supplies analytic carriers for a subset of those faces. A Fill feature's Boundary field selects the sketch by both entity reference and persistent key. Native line programs of that sketch can form closed directed components in the sketch chart, for example two triangles from six lines. The source endpoint references select the shared point records. The chart lifts each component to the vertices of one sheet in the STEP export of the same model, with line-to-point residuals at or below `0.000000942 mm` and sheet areas that agree within `0.00000001 mm²`. A further line in the same sketch can extend beyond the components and has a different management-flag field. The decoder transfers independent closed components as sheet bodies and reports `topology.sheet-components-inferred` for member exclusion and result grouping.

**Need.** Establish the management-flag and construction-state meanings and the Fill result grouping. Transfer the source-selected chart and directed boundary uses with their identity encoding.

**Conflict.** The current owner-first head interpretation groups records by runtime class. The paired definition suffix identifies that class; the next head operand selects the owner. Compact references and escaped `80` references use different identity encodings. Applying a one-based correction to the escaped form selects the wrong owner or a sketch itself.

### DI-30. Source-schema selector indexing

**Question.** Which source-schema identity and catalog index does each compact or fixed-width selector use, and how does a selected value program bind to its owner?

**Known.** `catia.md` §7.3 defines the current fixed-width selector lookup and the value-chain productions. Tolerance scalar candidates can carry compact definition bytes `F3 F4`. Reading each byte as a one-based identity selects the entries `LENGTH` and `TolNumValue` of the associated catalog. Under the same interpretation, a fixed selector `324` on a tolerance scalar selects `RefTolNumValue`.

**Need.** Resolve complete selector and value-program framing across definition, value, and suffix fields. Preserve each decoded scalar and its source owner when changing a lookup. Establish contextual entity-reference encoding independently of schema indexing.

**Conflict.** The current lookup selects the catalog entry at the stored fixed-width ordinal. The one-based interpretation selects the preceding entry and gives scalar-compatible roles for these controls. A partial change also alters the number of selectors in a definition and changes downstream value-chain admission.

### DI-31. Non-semantic feature control frame content

**Question.** Which fields of a `CATTPSNonSemanticGDT` annotation and its drafting object graph give the tolerance characteristic, the magnitude with its unit, and the datum references?

**Known.** A general-note frame such as `profile .06 | A | B | C` has one `CATTPSNonSemanticGDT` entity and no `CATTPSToleranceWithDRF`, `CATTPSToleranceWithoutDRF`, or `TolBloc` production; the typed tolerances and their secondary values account for every other tolerance of the STEP export. No source field stores the magnitude as binary64 in either unit (`0.06` in or `1.524` mm) or as binary32 `0.06`; binary32 `1.524` occurrences lie outside every entity record. The drafting catalog stores the frame as consecutive entries `DftGDT`, `ToleranceType`, `BoxNumber`, `UpperText`, `StringList`, and RTF cells whose texts are the magnitude and datum labels of the frame (`.06`, `A`, `B`, and `C`). No decoded field references the cell entries. The `StringList` object record has an empty payload, and the records of three consecutive entities have no decoded head. The unit is stated only as general-note text, for example `UNITS: INCHES`.

**Need.** A typed characteristic, a numeric magnitude with a typed unit, and an owner relation from the annotation to its cells transfer the frame as a semantic tolerance. Without them the frame stays native, and its tolerance is absent from the transferred tolerances.

### DI-32. `E7` and `E8` literals beside a contradicting formula

**Question.** Does a stored `E7` or `E8` parameter literal hold the value one or zero when the formula that reads or defines that parameter gives another value?

**Known.** `catia.md` §7.3 gives `E7` as the value one and `E8` as the value zero in parameter, constraint-range, and `Range` nominal suffixes. The decoder publishes every `E7` or `E8` length, angle, real, or integer parameter value and every `E7` or `E8` `Range` nominal as one or zero and charges `history.parameter-literal-assumed` with their count. A Boolean or string parameter with `E7` or `E8` has no value. The legacy scalar form reads `E7` as unset. An `E7` or `E8` input leaves its formula unevaluated. An `E7` or `E8` result whose evaluated expression gives another value keeps no value, and the same loss counts it.

**Need.** A saved part whose `E7` or `E8` parameters have known values in the application, with formulas that read and define them.

### DI-33. Evaluation order of `and` and `or`

**Question.** Does CATIA evaluate the right operand of `and` when the left operand is false, and the right operand of `or` when the left operand is true?

**Known.** `catia.md` §7.3 gives both operators as not evaluating the right operand in those cases. The decoder checks a stored formula result under that reading, so a formula whose left operand decides the result and whose right operand fails at the stored inputs keeps its text and its stored value. CADIR v11 §5.13 translates CATIA's `and` and `or` as themselves until the order is settled. Under that translation, the neutral expression of such a formula fails at the stored inputs, and the parameter keeps the stored value as its cache.

**Need.** A saved part whose formula has an `and` or `or` with a left operand that decides the result and a right operand that fails at the saved inputs, for example a division by an input whose saved value is zero: compare the formula's stored result with the decided value. A stored result equal to the decided value confirms the reading of `catia.md` §7.3.

## 3. Standard nested `V5_CFV2`

### SN-02. Rolling-ball scalar-jet semantics and tail control

**Question.** What does the explicit A-family rolling-ball scalar jet parameterize, which incidence identifies its target, and what does the tail control `03` or `05` mean?

**Known.** `catia.md` §6.2 "The rolling-ball tail starts after the knot lane" defines the complete 79-byte, 87-byte, 89-byte, `87 + 24*K`-byte, and `89 + 24*K`-byte productions. The scalar jet contains no curve identity. Its sampled values fit class-`15` parameter domains and include exact source knots. Several source curves can have overlapping domains, so containment cannot select a target. The absent-jet branch supplies no map. Both raw controls remain separate from neutral orientation.

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

Treating consecutive operand values as one parameter representation can join two consecutive source operands whose stored source intervals differ, for example approximately `[-58.658, 58.658]` and `[0, 0.901]`. The adjacent use-pair framing supplies no parameter transform between those operands. Two candidate map-to-side matches can have matching source intervals and adjacent face carriers, with chart-side operands `2` and `6` and map source operands that differ by 9. The chart-side-to-source offsets of the two matches then differ by 5, so a single additive offset cannot hold both matches. The chart-side frames and the distinct source-scoped records selected by resolved edge endpoint references form disjoint sets.

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

**Conflict.** A unique class-`19` record and a pcurve whose `op1` equals that record's tag can occupy the same source extent while their U intervals differ and the pcurve is not constant in V. Two distinct tags can lift to the same five-site geometric locus through different cylinder charts. Same-source containment, numeric uniqueness, and geometric coincidence do not identify the owning factory or object.

**Note.** Class-`5f` and class-`62` owner references can bind two class-`20` jets to consecutive spherical edge uses. Each jet interval equals its class-`25` source interval. The class-`06` compressed station controls need an independent decoding.

### SN-15. Additional standard-conic branch relations

**Question.** Which native incidence or chart relation selects a standard conic's edge interval when the admitted witnesses in `catia.md` §5.8 leave the branch unresolved?

**Known.** `catia.md` §5.8 "Cylinder and torus payloads use" identifies their witness point as the trimmed-face AABB center and defines a branch when one candidate's exact extrema fit all available incident-face boxes. The same bounds block exists on plane, cone, and sphere records. A box center is a spatial bound, independent of an edge's angular interval.

**Note.** Ellipse trims retained under the bounds-excess inference in §5.8 have reported excesses from 0.003397 to 0.004548 mm. Independent interior-arc verification remains required to settle this relation.

**Need.** Establish a source-bound parameter or boundary incidence for the remaining unresolved branches. Independently verify interior points of the inferred ellipse arcs and agreement between incident carriers to settle the bounds-excess inference recorded in §5.8.

**Conflict.** A cylinder face's AABB-center azimuth can lie about `1.38e-7` radians beyond an edge endpoint. Strict angular containment then selects the complementary arc, whose extrema exceed the face box by more than 3 mm. The native face box and the short arc of the STEP export of the same model select the other interval.

### SN-17. Unbound fixed-nine carrier context

**Question.** Which source-closed relation supplies carrier context for a fixed-nine parameter rectangle when the seven-record owner chart is absent?

**Known.** `catia.md` §6.4 "A fixed-nine numeric tail stores" defines the inline parameter rectangle and its separate carrier context. Tagged alternating and width-coded numeric tails also store a conservative Cartesian envelope of the limited surface section. The local backward-reference walk resolves topology allocations; it does not select an identity-free carrier. The admitted seven-record chart provides an exact carrier relation, independently of the reference dialect.

**Need.** Identify another source-closed carrier relation before evaluating an unbound rectangle as owned geometry. Preserve the parameter and Cartesian lanes independently of whether that relation resolves.

**Conflict.** Each tagged packet admits at least one A5 geometry candidate under domain, corner, and 25-by-25 interior checks, and a packet can admit several candidates. Candidate uniqueness supplies no serialized identity. An alternative carrier can pass the corner-box test but fail at interior sites. Its U domain can also end 0.0010119222600337707 parameter units below the stored rectangle; the interior comparison uses the common domain. Neither numeric proximity nor envelope containment assigns a carrier.

**Note.** The all-compact face-boundary predicate remains separate. Tagged alternating and width-coded envelopes do not bind a standard face by its AABB or by source position.

### SN-18. Eight-reference construction identities

**Question.** Which objects do the last three references of an eight-reference class-`37` construction select, and which source relation proves their geometric roles?

**Known.** `catia.md` §6.4 "An eight-reference B-family" defines the complete reference program, both control tuples, zero lane, terminal, and source-local rolling-ball carrier relation. Ordinary local owner/node joins use the checked successor relation; closed four-edge preludes use the separate ninth-identity-plus-ten relation. Neither local relation supplies a standard face ordinal. Standard face order belongs to the independent surface roster, whose complete rows contain no owner or node identity field.

**Need.** Resolve the referenced objects in their actual allocation or factory namespace and verify their geometry against the rolling-ball center and limiting loci before assigning those roles. Preserve every unassigned reference and addressing form.

**Conflict.** The public generic-fillet description lists center points, first limit, and second limit. The serialized jet stores first limit, second limit, and center instead. Public constructor descriptions therefore do not establish bridge wire order. Three references of one construction can follow the numeric pattern `[base+3,base,base+2]` and have no retained outer alias-row target; that pattern alone supplies no locus identity.

**Note.** Joint adjacency refinement of independent standard and STEP topology can assign every face of a part uniquely. A face graph with indistinguishable face pairs admits an exact graph isomorphism without a unique face assignment. Those geometric/topological associations do not turn a local owner integer into a standard ordinal.

### SN-23. Class-`0x61` roster controls and member identities

**Question.** Which owning contexts select roster controls `3` and `8`, and which native relation selects the current result from multiple valid body roots?

**Known.** `catia.md` §6.4 "Member bits retain serialized occurrence order" defines the reference forms, packed control, occurrence bits, and closure. Source-closed class-`65`/`60`/`61` chains with a one-child class-`60` owner use control `1`. A one-child allocation list does not by itself identify the owning domain. The wide control-`1` roster binds its members through the allocation widths in the frame headers. Control `3` identifies the sheet-group roster form in §6.4, with member orientation established in the native rolling-ball chart. Other carrier charts use inferred parity. The owning domain of control `8` remains open.

`catia.md` §3.1 "A `CATSeeBodyCont` visible-body list" defines the visible result-body selection through absolute allocation ends in the declared `CGMGeom` stream.

**Need.** Resolve result selection in other container forms. Verify chart parity for additional carrier forms. Establish a selector oracle for control `8`.

**Conflict.** A source-local roster and the current shell can have different cardinalities, for example 53 members and 700 faces. Descending numeric reference order agrees with independent face-orientation sequences, but numeric order alone does not supply an allocation-to-standard identity relation.

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

**Known.** `catia.md` §6.4 "A fixed-nine packet has a closed owner-boundary cycle" defines the typed cycle and its endpoint closure. The fixed-nine and counted-owner grammars preserve distinct reference forms and tails. The counted numeric-tail and adjacent A5 chart production constrain a standard face carrier through its stored bounds and vertices. The class-`5b` interval prefix and class-`5c` parallel reference/item lane have bounded structure; neither assigns a standard support identity. A contiguous ten-record boundary block closes under the five-reference edge grammar or uses the separate four-reference edge production. A closed four-edge graph with nine allocation references has a five-byte or a 62-byte tail. A graph with a 62-byte tail is retained by the fixed-nine native route or uses the counted allocation-reference grammar. These populations have different admission rules.

**Need.** Establish a source-closed allocation bridge to standard support identities. Assign the class-`5b` interval frame and suffix, class-`5c` item and terminal meanings, and the remaining owner-tail controls from an independent semantic relation. Broader counted-packet cycle projection requires its own complete endpoint and source-closure rules.

**Conflict.** The proposed universal face-node offset `target − final_owner_reference = m + 1` for a second owned-child operand `m ≥ 2` has source counterexamples: `m=2` with difference `0`, and `m=6` with difference `−7`. The bounded `27 03`/`27 05` prelude with difference `+10` remains a separate admitted relation. Numerical overlap across source namespaces does not establish an identity join.

**Note.** Existing native retention of copied control frames and copied owner frames passes full model checking. The existing typed boundary cycles match independently reconstructed source endpoints. Unassigned meanings remain open; no cross-source geometry binding follows from these structural results.

### SN-38. Two-edge sheet face orientation

**Question.** Which native relation assigns the absolute face sense of a closed two-edge sheet relative to its surface chart?

**Known.** The class-`62` complement relation determines the endpoint pairing of isolated closed two-edge faces. In the CATIA STEP export, a curved face of this form has forward sense and a selector-`d8` planar face has reversed sense. Planar face boundaries and areas agree with native pcurve lifts independently of their sense. The last singleton class-`61` record before the boundary owner has occurrence bit one on the curved face and bit zero on a selector-`d8` planar face; a tilted planar face has bit one.

**Need.** Prove the last singleton class-`61` to selected-face identity beyond an isolated one-face source and establish its chart-parity rule for additional surface carriers. A tilted planar face can also use carrier and pcurve productions outside the currently decoded variants.

### SN-41. Trimmed-face box extent

**Question.** Which construction produces the stored trimmed-face AABB of a curved face, and what bounds its distance from the exact face extent?

**Known.** `catia.md` §5.8 "A standard circle without an admitted native edge support" retains a witnessed ellipse interval whose extrema exceed a face box beyond the `2e-3` mm allowance when its complement exceeds that box by more, and charges `geometry.conic-branch-bounds-inferred`. On standard faces, the box exceeds the exact face extent by at most binary32 rounding except on a few faces, and it can lie inside the extent by more than `2e-3` mm. The following readings are falsified:

- The box is the exact trimmed extent: cylinder faces can fall short by more than `2e-3` mm.
- The box covers only the boundary vertices: half-cylinder faces whose four vertices lie in one axial plane reach `11.1091` mm from that plane at radius `11.1125` mm.
- The box is a control-net or conservative bound: it never exceeds the exact extent.
- The shortfall is binary32 rounding: `3.4e-3` mm is several hundred ulps at the coordinate scale.
- The shortfall follows a fixed angular step: under one tolerance, a face of radius `2.197` mm falls short by `7.3e-3` mm and a face of radius `11.1125` mm by `3.4e-3` mm.
- The shortfall is bounded by `0.01` mm: the per-document peaks fall in two groups, `0.0034`–`0.0100` mm and `0.020`–`0.26` mm.
- The bound scales with part size: in the first group the peak stays at or below `0.0100` mm for part sizes from `4.8` to `780` mm.
- The saving release selects the bound: documents saved by the same release fall in both groups.

The two groups match a tessellation chord tolerance that differs between documents. The trim mesh stores handles without coordinates, so a mesh-vertex box has no direct check.

**Need.** A stored or derived per-file tolerance replaces the smaller-excess rule with a containment test, and removes the loss that rule records.

## 4. Object stream

### OS-02. Class-`0x5f` terminal controls

**Question.** What is the semantic difference between terminal controls `03` and `05` in a `b5 03 5f` face?

**Known.** `catia.md` §6.7 "`b5 03 5f` (per-face node)" defines both terminal forms and retains them independently of loop location and face orientation. On outer loops with complete line-only pcurves, the terminal can contradict a direct terminal-to-winding assignment, and the exterior loop control agrees with every measured winding. The terminal value can also differ from a referenced loop's primary control. These are different control fields.

**Need.** Establish the terminal's writer-level meaning and selection rule through an independent source relation. Shared byte encodings alone do not assign allocation-addressing semantics.

**Conflict.** A retained quote of a face frame can name face and loop identities that are each 4 less than the identities in the source frame. The terminal is `05` in both descriptions. The source bytes govern the identity witness.

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

**Known.** `catia.md` §9 "The E5 `0xaa` current-limit program" defines the limit and affine fields. Class `0xaa` has one extrapolation control. The E7 surface program has three flags, eight continuation scalars and six suffix controls. Zero controls disable extrapolation. Sheet-owned tensor suffixes retain two compact descriptors and optional counts. Unapplied descriptors, flags, and continuation scalars are reported with `geometry.surface-extrapolation-not-transferred`.

**Need.** We must identify an active branch before interpreting its continuation fields. Establish the knot remap, reversal, and restriction for descending or changed active limits before admitting new chart-changing forms.

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

**Note.** A `FeatureForColor` group can carry a black-alpha-127 asset, selector atom `1`, and UUID-named atom `12` while all its governing FBB rows store a pale-blue face color. In the AP242 export of the same model, the black style reaches annotation-curve and tessellated-annotation occurrences and zero advanced faces. These groups supply no face-selector witness.

### RW-01. Extended rational weight-row controls

**Question.** Which non-palindromic A5 weight-row controls select reflected columns and leading repeats?

**Known.** Controls `0003` and `0007` in command `01` expand a complete symmetric row. Additional command-`01` controls and command `05` admit finite row programs under a reflected-column mask and leading-repeat reading. The decoder reports `geometry.surface-weights-inferred` when it emits that reading.

**Need.** Independently authored rational surfaces that distinguish each mask bit, leading-repeat count, and consumed scalar count.

**Conflict.** The extended controls previously failed the row-prefix gate. No decoded rational NURBS surface validates these extended readings.

**Note.** The legacy symmetric and flat-grid forms retain their established readings.
