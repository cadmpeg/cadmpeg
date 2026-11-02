# Autodesk Inventor IPT/IAM/IDW: Open Items

This document lists unknown Inventor byte semantics. [inventor.md](inventor.md) contains the settled format model.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. RSe selection and metadata

### RS-01. Multiple coherent databases

**Question.** Which typed field selects one database when several `V<n>/RSeDb` streams have schema 31 but differ in identity or save state?

**Known.** The storage-band number and database schema are independent. The decoder parses all database candidates and requires one coherent registry grammar.

**Need.** We must select one database without using path order or a save-version string.

**Note.** The name `coherent_schema` resolves to nothing in the crate. `RseInventory::build` parses every V-band `RSeDb` with the schema-31 grammar, leaving each parsed, unframed, or unreadable, and `DialectRecovery::classify` admits the document only when every declared schema is 31 and every body framed. Which parsed database supplies identity and save state when several differ remains unselected.

### RS-02. Metadata sections 5 and 6

**Question.** What item grammar and discriminator semantics apply to metadata sections 5 and 6?

**Known.** The backward spans establish their exact ranges and their join to section 4.

**Need.** We must type their records and references.

### RS-03. Section 7 long item form

**Question.** What fields select and define the section 7 item form whose item width is at least 76 bytes?

**Known.** The short section 7 form uses 32-byte items. Both forms have exact backward-framed ranges.

**Need.** We must validate the long item sequence without accepting arbitrary payload widths.

### RS-04. Revision and record-trailer variants

**Question.** Which revision-table versions, extended record-trailer property types, and trailer list forms govern RSe database variants outside the supported branch?

**Known.** The current envelope parses revision-table version 3 and the implemented trailer property and reference-list forms. Unsupported revision versions and trailer property types are refused after their owning record boundary is established.

**Need.** Each revision and trailer variant requires an exact field grammar, range rule, and native retention policy before semantic decode.

**Note.** `parse_revisions` treats every nonzero `kind == u16::MAX` payload selector as the short form and zero as the long form. It does not establish whether other selector values are valid or whether the selector has this polarity.

### RS-05. Registry object and node-count ownership

**Question.** Which registry object owns the count of node records that follow an entry, and what consistency rule applies when an entry contains more than one object?

**Known.** Each registry object stores a `node_count`. The current grammar uses the last object's value minus one as the entry's following node-record count. It does not require the object values to agree or identify the count-owning object.

**Need.** The count owner, bias, and multi-object consistency rule must be exact before the value can frame the following node records.

## 2. Kernel ownership

### KE-01. Multiple part carriers

**Question.** Which typed ownership and history references select the active model when a part contains more than one `PmBRep` segment or kernel-carrier record?

**Known.** The supported envelope has one `PmBRep` segment and one typed kernel-carrier record. Carrier signatures do not establish ownership.

**Need.** We must select active and historical carriers without appending all geometry.

### KE-02. ACIS save-format and carrier-footer bands

**Question.** Which ACIS save-format bands outside 217/218 and which segment-version carrier footers use a compatible SAB grammar and active-carrier semantic contract?

**Known.** ACIS 217 and 218 are the verified bands of the 32-bit header and SAB decoder. Other ACIS save-format bands take the same framing and record decode, carry an `Admission::Unverified` kernel layer with a `source.dialect-unverified` charge, and retain the carrier bytes when they read no geometry. The carrier footer is 17 bytes through segment major version 22 and 18 bytes from version 23. Segment versions below 15 read with the 17-byte footer as nearest-footer recovery. A carrier without ASM or ACIS signature bytes, or with an unframed footer, is malformed.

**Need.** Each additional ACIS or footer band requires direct-carrier framing, geometry, validation, and wrapper-parity evidence before activation.

**Note.** The 217/218 ACIS band branch was added with a shared implementation and synthetic 217/218 fixtures. It leaves the grammar for other ACIS bands and the remaining carrier-footer variants open. The item stays open with the narrower scope above.

### KE-03. Pre-15 carrier footer recovery

**Question.** Which segment major versions below 15 share the 17-byte carrier footer layout, and which pre-15 carriers differ?

**Known.** `parse_carrier` reads a 17-byte footer for segment major versions 0 through 22 and an 18-byte footer from version 23. A version-14 carrier with 217/218-band kernel bytes therefore frames and decodes the same records as the version-15 form. A version-14 carrier without an ASM or ACIS signature is malformed.

**Need.** A real pre-15 carrier requires its exact footer layout before the recovery rule can transfer geometry from versions below 15.

## 3. Product structure

### AS-01. Local prototypes and parent records

**Question.** Which record families define embedded local prototypes and parent-child placement inside one document?

**Known.** A `UFRxDoc` occurrence joins its external reference through its `reference_id`, checked against its first-section document identifier, and joins `AmDc` and `AmGraphics` through the exact occurrence identifier. Current-document external occurrences are root placements. A referenced assembly remains one unresolved external prototype because the codec does not load it. External-reference state bit `0x2000` marks a suppressed occurrence.

The schema-15 representation/model-state branch and its occurrence table are framed exactly. Unsupported property and export tags stop semantic transfer without inventing a record boundary.

**Need.** We must establish local component definitions and parent links from typed records before emitting non-root or local-prototype occurrences.

### AS-03. UFRx schema and occurrence-tag branches

**Question.** Which UFRxDoc schemas outside 11 through 15 and which occurrence property/export tags have stable field grammars and semantic roles?

**Known.** Schemas 11 through 15 and the supported occurrence/export tag sets are framed. Other schemas and tags are refused as unsupported semantic branches; their containing UFRx records remain bounded and are reported without inventing fields.

Pre-schema-15 level-of-detail records are framed, but their member semantics are not transferred: kind 1 members and the kind 2 subassembly level-of-detail selection are retained natively. Which members a level of detail suppresses, and how the active level of detail is chosen, is unproven.

Some assembly saves, for example weldment templates, store occurrence-table records that do not follow the occurrence grammar; they stay malformed UFRx records. The schema-15 header grammar of an assembly with more than the default model state or level of detail is unknown. The u16 state before the design-view count and the u16 pair after the active design-view name have no known meaning.

The meaning of the eight export-entry state bytes, of the export padding byte, and of first-section tag `0x1e` (a second name that usually equals the component name) is unknown.

**Need.** Each additional schema or tag requires exact field widths, repeated-tag rules, identity joins, and loss behavior before semantic transfer.

**Note.** `parse_occurrence_export` chooses export branches from the first two u32 values and numeric thresholds (`count`, `next`, and `0xffff`) rather than a typed discriminator. A valid single-entry or future export form in the same numeric range can be consumed as the empty or alternate form.

### AS-02. Transform units and exceptional branches

**Question.** What semantics apply to non-active placement branches, scale, mirrored state, and singular matrices?

**Known.** The active `AmGraphics` branch stores a compact finite 4-by-4 transform and joins it to an `AmDc` occurrence by exact identifier. Placement translations use centimetres and are converted to neutral millimetres. No field in the external-reference prefix supplies an active placement.

**Need.** We must type the exceptional branches before transferring their scale, reflection, or singular-transform semantics.

### AS-04. Occurrence state outside root visibility and grounding

**Question.** Which fields give the visibility of an occurrence more than one level below a root occurrence? Where is a hide stored when the subassembly's own design view sets it? What do the occurrence property tags `0x0f` and `0x10`, the `AmDc` occurrence flag bits `0x00400000` and `0x00800000`, and the state records of types `2133049bd21187a9a0002ea976d456c9` and `553a6b21d31123656000f587346bfbb0` encode?

**Known.** Section 9 of [inventor.md](inventor.md) states the hidden state of a root occurrence and of an occurrence one level below a root occurrence, in the active design-view representation, and the grounded state of an occurrence: an occurrence is grounded when a related state record carries flag bit `0x02000000` (commit `b9e57711aa`, #496). The decoder keeps a key path of more than one key natively and hides no root occurrence.

When a part inside a subassembly is hidden from the top assembly, the editor can either store the hide in the top assembly or write it into the design view of the subassembly. In the first case the top assembly stores a visibility-off record that holds the key path of the subassembly occurrence and the counted name `Default` of the design view that the occurrence follows. A representation record also grows by 12 bytes. Those 12 bytes are unassigned. The second case stays native.

Tag `0x0f` and tag `0x10` of the first `UFRxDoc` occurrence property section change on one occurrence while all other occurrence, `AmDc`, and `AmGraphics` fields stay the same. Neither tag stores the hidden or the grounded state: the `UFRxDoc` occurrence record is the same when either state changes. Tag `0x0f` stores values other than 0 and 1, for example `0x2a`, `0x37`, `0xd3`, and `0xff`. `AmDc` occurrence flag bits `0x00400000` and `0x00800000` change with no proven role. Bit `0x10` occurs only together with one of them.

Records of type `553a6b21d31123656000f587346bfbb0` in the `AmDc` segment start with the same key list as a visibility-off record and then store floating-point values and a counted UTF-16LE string. Records of type `2133049bd21187a9a0002ea976d456c9` store small state values. Their join to a representation and the meaning of their values are unknown.

**Need.** We must find the field for each state, and prove each value with two documents that differ only in that state, before we set `visible` for an occurrence two or more levels below the root, or for a hide that the subassembly's design view stores.

## 4. Properties, materials, and design intent

### PR-01. OLE property variants and code pages

**Question.** Which additional OLE code pages, property value types, section-directory variants, and preview payloads occur in Inventor documents, and what are their exact mappings?

**Known.** The current parser handles the supported little-endian property-set header, section ranges, scalar/vector/FILETIME/BLOB/clipboard forms, UTF-16, and the admitted `encoding_rs` code-page set. Unknown typed values remain native-only; an unsupported code page or property variant stops typed property projection after its stream has been identified.

**Need.** Additional code pages, typed variants, and preview encodings require exact decoding, range validation, and neutral/native mapping rules before transfer.

**Note.** When a property section has no code-page property, LPSTR and dictionary strings are decoded as Windows-1252. The format model does not establish that an absent code page selects Windows-1252.

### PR-02. Metadata property identity and conflict precedence

**Question.** Which OLE property-set identity and precedence rule select title, author, description, part number, and document kind when multiple sections expose matching names or built-in IDs?

**Known.** `MetadataProjection` accepts normalized names across property sets. It keeps the first nonempty title, author, description, or part number, but replaces the document kind whenever another parseable value is encountered. The traversal order is the only selection rule.

**Need.** A canonical FMTID/property identity and conflict rule are required before metadata can select document semantics or overwrite a neutral value.

### MA-01. Additional face appearance styles

**Question.** Which `PmGraphics` or `FBAttribute` style families bind a Protein appearance asset, texture, or non-diffuse presentation channel to a face?

**Known.** The `PmApp` document-default record selects one rendering-style record by a carrier-local one-based record reference. The rendering style stores the Protein asset GUID and asset-library identifier. Their unique catalog join supplies the default appearance for every part body. A PmGraphics face joins a transferred ASM face through the shared nonnegative Design key. Its object-style collection can select one primary-color style whose second RGBA vector is a direct diffuse face override. The face override has precedence over the body default.

**Need.** Each additional style family requires an exact owner path, channel semantics, and precedence before transfer.

**Note.** The implementation selects `PmGraphicsPrimaryColorStyle.colors[1]` as the diffuse face color without a record discriminator for the four color slots. The slot role remains part of the open channel-semantics question.

### MA-02. Protein material-property identity and precedence

**Question.** Which schema-qualified Protein property owns each material scalar, bitmap URI, bitmap URN, base-color channel, and texture-mapping value, and what value does each schema define when a property is absent?

**Known.** Material projection scans matching property IDs and accepts the first match for texture mapping values, bitmap paths, bitmap URNs, and normalized color candidates. It also maps `generic_refraction_index` and `transparent_refraction_index` to one neutral key, so a later map insertion replaces the earlier value.

**Need.** Schema declarations must establish property ownership, channel precedence, ambiguity behavior, and omission semantics before material values can be transferred.

**Note.** The current projection supplies schema-independent values when mapping properties are absent: map channel 1, UVW source 0, zero offsets and rotation, unit scale, enabled repeat, zero real-world mapping values and bump depth, and unit normal scale. These values are not yet established as the omission semantics of each Protein schema.

### DE-01. Feature and history record graph

**Question.** Which typed records define history order, feature operands, suppression, and result-body transitions?

**Known.** Section 13.8 defines the Tagging property vector in segment major versions 19, 23, and 31 and its surface-body selection. Model roots, root labels, and group labels define the browser history order, the end-of-part position, and the flat-pattern history. Content-header flag bits define suppression and the below-end-of-part state. Feature labels identify consumed sketches, and pattern-feature participants identify seed features. Numeric PmDc parameters, planar sketches, generic feature records, rectangular-pattern records, mirror records, and end-of-features records have stable carrier-qualified identities. Combine has exact target and tool selections, a Boolean operation, keep-tools state, and a retained target result identity. Generic, rectangular-pattern, and mirror records supply ordered property-reference vectors. Pattern records also supply ordered participant references. Rectangular patterns have fixed-spacing feature seeds, one or two directions, count and spacing joins, cached line selections, and a separate instance-suppression list. Stored-body Base features identify their stored source and result surface-body wrappers. Surface-body wrappers with nonzero unqualified references have the current-model body join in section 13.4 through metadata external references, the active carrier, and its body key. Typed feature properties identify part-operation, extent, hole, fillet, chamfer, Boolean, boundary-patch, feature-dimension, object-collection, fillet-edge-set, edge-collection, face-collection, selection-list, edge-item, body, profile-selection, and placement records. Closed one-sided, symmetric, two-sided, to, and from-to extrusion, with to extents that end at a work plane, a plane face, or a work point, drilled/countersink/counterbore/spotface holes with fixed-depth, through-next, through-all, work-plane, and native face targets, constant-radius edge-fillet, equal-distance edge-chamfer, and ten-slot shell branches have exact dimensions, native selections, placements, and native result-body identities. Full revolutions and positive one-sided fixed-angle revolutions about work axes have exact operands with the controls specified in section 13. A null revolve slot 11 adds no profile operand; slot 1 selects the stored profile. Identity-frame, zero-option mirrors have the complete 13-slot operands and extension controls specified in section 13. The segment-major-19 body-replacement form with parent flags `0x6200`, true slots 6 and 7, false slot 11, and one slot-10 source reflects the selected body in place. Feature mode has ordered earlier seed features and result bodies; body mode with no instance map uses one collection for source and result body identities. The segment-major-19 body-copy branch has one source-body mapping and a generated result node as specified in section 13. Feature-label records supply an owner, a label, a class identifier, and ordered participant references. Entity-style links retain associative entity identifiers and entity types. Planar point, line, circle, ellipse, placement, geometric-constraint, and dimensional-constraint graphs have exact record references and neutral identities. Single-transform Move, MoveBody, and Rotate body motions and the translation branch of Move face motion have exact selections, rigid transforms, and persistent result-body identities. Exact RSe frames provide type identifiers and record ranges for the remaining design records. Thread annotations have nine typed property slots, a native face selection, and a thread-definition record in segment major versions 19 and 21 with nominal size, designation, standard, class, and pitch fields. Section 13 defines the exact metric and Unified sizing forms. The meanings of Boolean slots 1, 2, and 6, numeric slots 3 and 4, the slot-5 edge selection, slot 7, the remaining thread-definition controls and table strings, and additional sizing forms remain unknown. The face-item selector address space and its join to input topology remain unknown. Section 13 defines the Thicken selection-wrapper and control-record grammars. The face collection holds one or more face items, and each face item holds one index value with the qualifier `0x10000000`. Thicken slots 3, 4, 7, 10, and 17 are false, and the slot-12 parameter has the value 10. The combinations of the slot-1 value, slot 2, and slot 16 are 3 with both true, 3 with slot 2 false and slot 16 true, 3 with both false, and 2 with slot 2 true and slot 16 false. Loft section lists define ordered native section selections. Section records link to a kernel entity, a condition, two numeric parameters, and an optional Boolean. Loft and sweep records have typed control enumerations and result references. Split properties have two typed enumerations and two Boolean records. The Split tool enumeration selects a work-plane reference or a surface-body wrapper. Work-plane browser nodes have history positions. A browser work plane with orthonormal u and v axes has an exact datum-plane frame; one with other axes stays native with `feature.work-plane-frame-native`. A solid split has one input body, a work-plane or surface tool, and two result bodies. Section 13.3 defines the version-19 selected-face split with one nonempty face item, a work-plane tool, and the persistent slot-7 result body. It also defines the version-19 surface-tool body trim with a persistent slot-7 result body and a separate surface-body tool.

**Need.** Revolve slots 6, 8, 13, 14, 15, 18, and 19 require their non-null meanings. Slot 9 requires values other than the type-4 value 0. Slots 10 and 12 require their non-default meanings. Slot 16 requires its control meaning. Slot 4 requires the symmetric total-angle convention and the second-side parameter join. Slot 5 requires the fixed-angle meaning of value 1 and the sidedness map for other values. Other revolve extents require their exact angular termination and second-side operands. The work-axis entity flags, two or three u32 values, reference list, optional `0x203` marker, and state byte require their meanings. The work-plane entity flags, u32 values, and optional `0x203` marker require their meanings. The parameter-dimension header fields, annotation item grammar, list metadata, and five annotation bytes require their meanings. The Thicken side and symmetric-distance rules outside the forward-offset tuple in section 13 require their Boolean and parameter joins. The offset construction in segment major versions other than 19 and 25 requires its distance and polarity rules. Slot-1 value 2 requires its cut construction. Other part-operation values require their construction and result joins. Slot-2 true forms, slot-16 false forms, other slot-12 values, and other type/value pairs in slots 13 and 14 require their construction meanings. Their selected input-face revisions, oriented normals, and named result faces require exact joins. Nonzero external-key extensions, section-7 forms other than segment pairs, Base results without a current-carrier join, body-level evaluation dependencies, nonzero rectangular-pattern computation and direction controls, true rectangular-pattern slot-11 values, non-null rectangular-pattern slots 3, 7, and 8, nonpositive rectangular-pattern spacing operands, unequal nominal and model words in rectangular-pattern integer variables, nonzero auxiliary selection controls and non-null auxiliary selection-control references, rectangular-pattern work-axis controls outside section 13.6, nonzero direct-direction flags, parameters, and extensions, cached direction states outside section 13.6, cached spans that disagree with their referenced work-axis spans, nonzero rectangular-pattern extension values outside section 13.6, nonidentity rectangular-pattern placements, non-line rectangular-pattern selections, cached-selection forms other than selection form 1, rectangular-pattern slots 12 through 17 from segment major version 21, rectangular-pattern body-selection joins, circular-pattern operands, non-identity mirror placement, nonempty mirror selection records and selection controls other than `0x30000004`, nonzero mirror options and variables, mirror dimension lists, mirror property lists, values, and control bytes outside section 13, mirror extension values outside section 13, mirror instance-map forms beyond the segment-major-19 single-entry branch, generated-body node flags beyond `0x6200`, and mirror slot-7 and slot-11 Boolean combinations outside the body-copy and body-replacement branches, body-replacement parent flags outside `0x6200`, the construction and result joins for mirrors of work-plane and other participant records outside earlier generic or pattern features under one model root, mirror work-plane controls outside section 13, extrusion boundary-patch operands outside the repeated-patch form, extrusion targets other than work planes, plane faces, and the work-point form in section 13, extrusion extent-7 targets with a null termination flag and no work-plane or work-point target, extrusion extent-7 targets with termination value 0, extrusion work points with entity flags other than 0, nonzero u32 values, or nonempty reference lists, work-point extrusion targets with oblique travel or points behind or on the profile plane, the termination flag outside the extrusion and hole target forms, the hole Booleans in slots 17, 26, and 27, native hole face-index joins, non-default fillet laws, fillet slot-1 and slot-10 operands, true fifth edge-set controls, fillet selection type `0x00010004`, false chamfer direction controls, face-fillet and full-round construction operands, support-side ownership and angle reference in oriented chamfers, Shell slots 0, 6, 7, and 8, non-null Shell slot 4, the Shell slot-10 extension, nonempty Shell selection lists, and the remaining operation families require exact record joins before transfer. Shell face references require a join to historical faces and input bodies. The Shell controls require rules for thickness variation, direction, offset construction, corner joins, and intersections. Nonempty section type-7 maps need an entry grammar. The edge expansion of fillet selection type `0x00020004` with value 0 and the meaning of null continuity outside sheet-metal corner rounds and true fillet continuity need rules before transfer. Loft construction flags, nonzero section conditions, section scalar roles, and auxiliary selections require exact rules outside the complete generator-owned result forms. Sweep slots 5, 6, 11, 12, 13, and 14 require their non-null operand roles. Sweep slot 2 requires the input-body selection and result-revision joins for cut, join, intersect, and surface operations. Slots 8 and 10 require construction and scaling rules outside their `(3, 0)` and `(2, 0)` type/value pairs. Slot 9 requires the orientation rule for type-1 values 0 and 1, other forms outside type 2 with value 0, and null controls. Slot 3 requires its taper or twist role, sign, unit, and path-parameter convention for nonzero values. Slot 4 requires the path extent map, termination operands, and travel direction outside its versioned type with value 3. Slot 7 requires the roles of additional dimensions and their parameter joins. Slot 15 requires historical result-body joins and complete generator-owned result partitions outside the circular solid form. Nonzero or unknown section conditions require a neutral representation. Closed lofts, ruled spans, simplified results, partial paths, variable scaling, and historical results require their own construction and topology joins. Noncircular and multi-loop sweep sections require exact section frames, loop orientation, side-face correspondence, and inner-loop result joins. Outside the complete keyed-wire circular form, sketch line, arc, and spline path chains and selected topology edges require complete curve intervals, chain order, travel direction, and frame transport through corners and closed paths. Other ordered generated-face naming forms require exact member identities and partition rules. Surface-loft part-operation value 5 requires its surface-body creation or combination rule. Surface-loft section operands that name an untyped participant require its record grammar and exact curve-selection join. The section `source_value` requires its selection role outside the positional solid form. Earlier surface-loft generated names require their section-order, closure, and side-face partition rules. A zero enumeration value or a false unnamed Boolean alone does not identify a construction rule. Split work-plane trims, surface-tool trims outside section 13.3, mode-2 forms outside section 13.3, the slot-6 Boolean outside the all-region split forms, the slot-1 Boolean outside the false-control forms, face-selection joins, and the correspondence of each solid-split result body to a side of the tool require exact rules. A work-plane trim with a null slot-9 reference requires an input-to-result revision join independent of a result collection. Surface-tool trims require their input-body and tool-body revision joins. Face-item selectors require their complete input-face revision and resulting face-region joins. Retained and removed sides require an oriented tool and an exact Boolean-to-side map. These fields must resolve before the score advances to L4. Multiple-transform body-motion lists require their transform-to-body correspondence. Other face-motion branches require their operands and axes. Surface-extrusion operation value 5 requires its body-creation or body-combination rule. Boolean slots 3, 7, 20, and 22 require their construction roles and polarity. The slot-14 surface-body reference requires its selection role. Open and trimmed selected wires require an exact curve-selection construction. The slot-16 surface-body reference requires its result-revision join. Direction and side rules require a join from the selected profile and termination operands to that result.

DeleteFace Boolean slots 1 and 2 require their construction roles and polarity. Its slot-3 body selection requires operand/result ownership and a body-revision join. Its slot-0 face items require selected-input-face revision joins.

Tagging slot 0 requires its input and result body revisions, its topology-naming effect, and its geometry-effect rule before a neutral construction or a non-modeling history node can represent it. Multiple slot-0 collection members require their body-order and naming rules. The Tagging state, outline value, property-list metadata, collection-list metadata, and other segment versions require their meanings.

### DE-02. Compound parameter units and function expressions

**Question.** Which PmDc unit graphs define compound dimensions, derived units, and offsets, and which functions do function-call codes other than 1, 2, and 3 name?

**Known.** A scalar parameter unit with one supported numerator, no denominator, and no derived-unit reference transfers with exact length, angle, or dimensionless semantics. Literal, parameter-reference, unary, and binary arithmetic expression records form a closed ordered graph. Function-call codes 1, 2, and 3 are cosine, sine, and tangent.

**Need.** Compound or derived units and other function codes require exact dimensional and textual semantics before neutral transfer.

### DE-03. Additional sketch geometry and relations

**Question.** What does the planar-sketch `count_value` count, what locus semantics do nonempty constraint maps encode outside the coincident, tangent, distance, circle-center, and midpoint solver hints, and which PmDc record forms define elliptical arcs, spline branches outside the closed clamped polynomial and endpoint-bound open cubic forms, offset curves, projected geometry, text, and pattern relations, what the `entity_flags` bits `0x20`, `0x100`, and `0x800` and the bits outside the resolved line, circle, ellipse, and spline sets mark, which operands define a circular arc when its endpoint points and stored radius disagree, what entity-flag bit `0x8000` of a line states beyond the doubled distance of its dimensions, what role a non-null midpoint parameter reference has, what midpoint locus applies to ellipse, spline, and helper operands, what the regular-polygon record's `value:u32` and the polygon relation's two trailing u32 values encode, what the tangent hint codes 1 and 2 state for a circular-arc operand, and which angle-dimension `flags` value selects the supplement of the line angle?

**Known.** Circular-arc records can reference endpoint points whose distance from the center differs from the stored radius. Midpoint records also carry non-null constraint-header parameter references. Planar-sketch member arrays reference types `013cbb1fd2114d68a0009e9a3c3aa076`, `0ead987ab9495ceefc186787a2a20643`, `7f341025ae4a79961d9047869d5f4e48`, `46737b31d3117c7a60001cb3d1c1fbb0`, and `845c3bbfd2112ae960004bb38932edb0`. Their RSe frames give exact payload boundaries. A closed ellipse has state 1 and two unqualified null leading point references. The `count_value` is the u32 field between sketch state and the type-8 reference array. Point, bounded-line, circle, circular-arc, closed-ellipse, and closed clamped periodic polynomial-spline and endpoint-bound open cubic entities form closed planar sketch graphs. Spline-point relations store ordered spline and point operands. An ellipse record with two leading point references is bounded. Profile wire bodies use curves whose `entity_flags` hold bits `0x40`, `0x1000`, and `0x40000`. Profile wires also use circles with bits `0x80000` and `0x200`, lines with bits `0x200` and `0x8000`, and splines with bit `0x4`. No profile wire uses a line with bit `0x80000` or `0x200000`. Bit `0x200000` occurs only on lines. Bits `0x20` and `0x800` occur only on curves that no profile wire uses; bit `0x20` occurs only on lines that have no neutral projection, and bit `0x800` occurs on curves that carry no constraint. One curve with bit `0x100` is a profile wire curve; other curves with bit `0x100` are not. A distance dimension with a line operand that carries bit `0x8000` states twice the distance between its operands. Coincident, parallel, perpendicular, tangent, horizontal, vertical, circle-center, equal-radius, equal-length, midpoint, fix, horizontal-alignment, vertical-alignment, collinear, symmetric, polygon, equal-distance, sketch-block, radius, diameter, distance, angle, horizontal-distance, and vertical-distance relations with empty constraint maps have exact operands. A tangent hint code keyed by a circular-arc operand does not change the tangent relation. Code 1 keyed by an arc occurs where the tangency is at the arc start; code 2 occurs where it is at the arc end and where it is inside the arc.

**Need.** A circular arc with inconsistent endpoint and radius operands requires an exact authoritative curve rule. The listed member classes require their complete record grammars and curve or relation operands. The count requires an exact referent before validation uses it. Nonempty constraint maps outside the coincident, tangent, distance, circle-center, and midpoint solver hints require exact locus semantics before neutral transfer. A tangent hint code 1 or 2 keyed by a circular-arc operand requires its exact meaning before the hint can place the tangency. The polygon record value and the polygon relation values require exact referents before a polygon transfers as more than its edges and relations. A non-null midpoint parameter reference requires its exact parameter and locus role. Midpoint relations outside line and circular-arc operands require their exact locus semantics. The spline-point relation requires its exact locus and parameter semantics. Outside the two defined polynomial forms, the spline u32 list, `knot_value`, `pole_value`, and remaining control values require exact referents. Spline control values outside the closed polynomial and endpoint-bound open cubic forms, unequal primary and secondary arrays, rational poles, other open curves, and mixed or trimmed spline profile rings require their exact rules. Each additional entity and relation requires its complete solved geometry, ordered operands, parameter role, and profile-boundary semantics. Bits `0x20`, `0x100`, and `0x800`, bit `0x200` of an ellipse or spline, bit `0x8000` of a circle, ellipse, or spline, and bit `0x200000` of a circle, ellipse, or spline require their exact construction meaning before the curves transfer as profile or construction geometry.

### DE-04. Parameter tables without a model owner

**Question.** Which record owns a PmDc parameter table whose context reference is null, and what scope applies to its parameters?

**Known.** These tables have flags `0x20` and two-entry member lists. No record references the table. Their parameters are referenced by records of type `b038b999d211c35b6000b3856c2387b0`. Their parameters remain native with a parameter-scope loss.

**Need.** The owning record and its feature or sketch identity must be exact before these parameters can enter a neutral scope.

### DE-05. Model-root and parameter-table fields

**Question.** What do the model-root flags, the model-root values other than values 3 and 9, the model-root `scale` and `terminal_value`, the parameter-table member list, and the parameter-table suffix encode?

**Known.** Value 3 references the root's parameter table and value 9 is null or repeats it. Value 12 is 1 in document model roots and 7 in flat-pattern model roots. The member list and the name map contain every parameter of the table and also other records. The suffix is retained by length.

**Need.** The model graph, the flat-pattern definition operands, and the table membership rule must be exact before the flat-pattern model can transfer more than its parameter scope.

### DE-06. Profile-selection controls and geometry

**Question.** What do these profile selections select: a selection with a nonzero `value`; a selection whose entity link is an entity-style link; a wire edge whose naming attribute has another integer layout; a wire edge whose associative identifier names no curve of the consumed sketch; a wire whose edges do not form one closed ring; and one ring selected twice in one boundary patch?

**Known.** A closed selection whose wire body edges each run along a whole curve or a part of a curve of the consumed sketch is one closed ring of that sketch. Some wire bodies name associative identifiers that the consumed sketch gives to points. In some wire bodies an edge names one curve of the consumed sketch and lies on another curve of that sketch. Some naming attributes store three integers after the five leading fields.

**Need.** Each form requires its exact sketch curves or region rule before the extrusion profile can transfer.

### DE-07. Other PmDc segment versions

**Question.** Which content-header, stamp, list, and record forms do PmDc segments of major versions 24, 26, and 27 use, what do content-header stamps encode, and what roles do the scalar pair and the operand references of a state-2 point have?

**Known.** Segment major versions 23, 25, and 28 through 31 use the header and record forms of the specification. A stamp is −1 or a nonnegative value; nonnegative stamps include values whose hundreds equal the segment major version. PmDc record decode admits segment major versions 15 through 23, 25, and 28 through 31; the records of other versions stay native. The content-header stamp rule reads one stamp in versions 23 and 24 and two stamps from version 25. The 24, 26, and 27 forms have neither admitted-band exercise nor real-carrier evidence. A state-2 point has two finite scalars and two entity references. The point position gives its solved coordinates.

**Need.** Each other version requires its header and record forms before its PmDc records decode. A stamp requires an exact referent before it can affect transfer. The scalar and operand roles require exact semantics before this association can define a neutral relation.

### DE-08. Sheet-metal construction controls

**Question.** Which construction roles do the relief and transition Booleans select? What are the construction rules outside the control form specified in section 13.5, and what shapes do the style relief and transition enumerations select? Which historical body revisions and output naming rules complete these operations?

**Known.** Section 13.5 defines every slot of the 17-property attached plate and the 11-property bend, the shared attachment, relief, transition, corner, and result references, their fixed property layouts, and the input-edge selection path. The document style stores separate default bend-relief and transition-form enumerations and the named length parameters listed in section 13.5. Form equality compares the enumeration type and value, independently of the length expression. The attachment contains thickness, radius, relief width, and minimum remnant. The relief contains width, depth, minimum remnant, and seam gap. The transition contains its radius. The corner control contains gap, size, and Jacobi radius. Six- and seven-property corners share the plate corner control, profile, and attachment selectors. Slot 1 of the six-property form repeats the surface-body property in plate slot 6; slot 1 is null in the seven-property form. The seven-property form has a result collection whose identity is distinct or shared with the plate; distinct collections can have the same member references. Each selection wrapper can contain several edge operands. The active single-edge branch in section 13.5 has an unchanged body when its corner source has no generated names across the completely framed carrier and archive. Each is a length parameter with an exact unit and formula graph. The flange owns separate ordered plate, bend, and corner generic-feature children. The bend shares the plate's attachment; it is not nested inside that property. Neither the bend property vector nor its attachment contains an angle parameter. The extended attachment ends with a reference to type `93596ba9d311efa86000b3a99dccefb0`. The input attachment selects an existing sheet edge through a wrapper, edge collection, edge item, and naming selector. Sheet-metal style parameters identify literal versus style-derived dimensions. The style supplies parameters named `Thickness`, `BendRadius`, `BendReliefWidth`, `BendReliefDepth`, `CornerReliefSize`, `MinimumRemnant`, `TransitionRadius`, `JacobiRadiusSize`, `GapSize`, `SeamGap`, and `KFactor`. A bend-free new-body plate has complete planar base-flange operands, including its profile, direction, thickness, Boolean sides, enumeration side 0, direct result body, and result-collection form. A false relief control occurs with and without plate-source `INV_NMX_BEND_TAG` faces. These face names do not identify the relief width and depth. Absence of a cut at a free bend end does not establish a disabled relief. The transition role of the flange and bend face-naming integers is unresolved. Equality of local and style forms does not identify the Boolean role.

**Need.** Nonempty plate slot-7 edge collections require their selected-edge role. The attachment controls outside section 13.5 require their construction map or an exact join to a document-style operand. Equality with an unrelated style Boolean is insufficient. Relief and transition forms that differ from their document-style forms require a byte-to-construction map. Relief and transition Boolean polarity requires a construction rule. Style-default relief and transition forms require their byte-to-shape maps; form equality alone supplies no neutral document rule. The attachment-state scalar requires its construction meaning. The version-19 style needs the transition-form and transition-radius operand positions. Slot-2 Boolean true requires a flange-specific thickness-side rule. A second attachment direction antiparallel or oblique to `normal × U` requires its construction rule and a named result-face join. The relation between an angular parameter named by the plate dimensions or auxiliary construction record and the bend angle requires its sign, supplement, and fold rules. Shared result-body wrappers require a revision-selection rule. Additional thickness-side values require their distribution about the sketch plane. Corner forms require their seam and relief construction map and their selected bend-region or plate-boundary pairing. A single owning bend feature can contain several selected edge operands. Outside the single-edge unchanged branch of section 13.5, absent corner names do not resolve the geometric effect. Multi-edge corners require exact bend-region pairing and the corner-form construction map. Their history positions require joins to body revisions before and after evaluation. A shared result-collection reference alone does not identify those revisions. The hem control requires form, position, fold-direction, dimension, and selection rules. Flat-pattern definitions require their construction operands.

### DE-09. Additional edge naming forms

**Question.** Which secondary naming arrays, additional selector forms, and naming families resolve edge items, and what does an empty edge-item index-reference list select?

**Known.** An edge-item index reference names a selector by its PmDc source index. A direct selector and an ASM matching attribute share a source index and an edge key. A generated attribute on the same edge supplies its generating feature and associative name. The archive can contain this edge after a later operation removes it from the active body. A selector's trailing edge-item reference selects its owning item, including an item whose index-reference list is empty. Section 13 defines the qualified direct selector with one or two generator indexes, an empty secondary array, a reciprocal explicit edge-item reference, one matched edge revision, and straight historical geometry.

**Need.** Additional selector forms, nonempty secondary naming arrays, matched-attribute qualifiers, generated roles, curved input revisions, naming families, and Boolean control tuples outside section 13 require exact operands and joins before neutral edge resolution. The reverse edge-item relation requires an operative-selector rule when the item's index-reference list is empty. Result-face names require an exact selected-input-edge join, an input-body revision, and edge-set ownership. Suppressed features require their stored selection independently of evaluated result faces. A generated source-name contribution alone does not supply these relations.

### DE-10. Sheet-metal generated names

**Question.** Which complete names identify the edges and faces created by an attached plate, its bend, and its corner treatment?

**Known.** `INV_NMX_FLANGE_TAG` and `INV_NMX_BEND_TAG` attributes name faces with the source indexes of plate, bend, or corner features. Their schema-44 prefix identifies the generating source index and owning entity. The schema-38/40/42 class-chain forms contain ordered source-name pairs and fixed-length family tails. The pair naming values, remaining tail integers, and the four-value INV_NMX_DUP key require their separate roles. The duplicate qualifier has no feature-source field; its owner has a separate generated source name. Existing-sheet input-edge selectors join ordinary generated names independently of the attached plate's result names. Section 13 defines those direct selector and generated-edge joins. A flange label has the consumed planar sketch, plate, bend, and corner as its four ordered participants. The parent flange has those same three generic-feature children.

**Need.** The complete flange and bend naming tuples, their integer roles, and the rule for an edge named through its adjacent faces must be exact before generated `local_id` values can be assigned. A new neutral feature must not assign an edge role from geometric proximity or from record order.

### DE-11. Derived-part source construction

**Question.** Which fields select the external source document and revision and define source update dependencies? Which component fields extend the version-19 and version-23 forms?

**Known.** Section 13.4 defines the version-19 and version-23 derived stored-body forms, their source and result wrappers, their component references, and their transform reference. The stored-body Base family includes this derived form. Its result uses the current-model body join independently of the component transform.

**Need.** Component references outside the transform require complete record grammars and source-document joins. Source revision selection and update dependencies require exact operands before external construction can replay. The additional component fields in other segment versions, and nonempty component lists, require their construction roles.

### DE-12. Extrusion termination face identity

**Question.** Which records join an extrusion's stored planar termination surface to its selected input-body face and history revision? What roles do the other surface reference, state, and value forms have?

**Known.** The zero-control face-surface form in section 13 stores three null references and a complete plane frame. The target source distinguishes this stored face surface from a work plane or work point. Its body-face identity stays native with `feature.extrusion-target-face-native`, once per face-surface start or termination.

**Need.** An exact selection-to-face join and input-body revision are required for a resolved face identity. Other surface controls require their selection, frame, and polarity roles.

### DE-13. Coil construction operands

**Question.** Which property operands define the Coil type, handedness, axial travel, taper, and start and end conditions?

**Known.** Section 13 identifies the Coil feature-label class and its generic feature owner. The ordered property vector retains references to operation, boundary-patch, work-axis, Boolean, numeric-parameter, dimension-list, and result-collection records through their existing grammars. Coil remains a native history operation with `feature.coil-operands-native`.

**Need.** Slot 0 requires the Coil input/result body construction joins for operations outside new-body creation. Slot 1 requires its selected-profile construction join. Slot 2 requires the axis control and orientation rules outside the zero-control work-axis form. Slot 3 requires its Boolean construction role and polarity. Slot 4 requires its Boolean role and its relation to axis direction. Slot 5 requires its control grammar and the type-selector map for pitch and revolution, revolution and height, pitch and height, and spiral. Slots 6 through 9, 11, 12, 14, and 15 require their driving and inactive parameter roles, sign conventions, and parameter dependencies. Boolean slots 10 and 13 require their construction roles and any joins to end conditions. The profile selection requires its consumed-sketch and axis joins. Result selections require their input/result revision joins. Each polarity requires a controlled positive case before a neutral helical sweep can represent it.

## 5. Adjacent envelopes

### EN-01. Other CFB and RSe versions

**Question.** Which RSe schema, metadata, record-trailer, and carrier variants accompany CFB version 4 or metadata versions other than 8?

**Known.** The compound reader admits valid CFB version 3 and 4 containers, and a DIFAT chain terminated by FREESECT only at its exact FAT-implied count. FREESECT terminators on directory, mini-FAT, and stream chains have not been observed. The Inventor semantic envelope is schema 31 and metadata version 8. Those grammars are applied to every document, whatever it declares; a declaration outside them leaves the document dialect-unverified.

**Need.** Each governing variant requires a separate finite support envelope.

### EN-02. Presentation documents

**Question.** Which segment families and records define IPN document semantics?

**Known.** Property metadata can name presentation documents.

**Need.** Presentation records require exact layouts and graph joins.

## 6. Drawing documents

### DI-01. Untyped drawing record classes

**Question.** What are the payload layout and drawing role of each drawing record class that has no typed form in section 15?

**Known.** RSe framing supplies each record's type identifier and exact payload boundary. The registry type of the owning segment gives its drawing family. Records of untyped classes are retained as native RSe records.

**Need.** Each class requires its byte grammar, its reference address spaces, and its semantic role.

### DI-02. Sheet paper and drawing-view placement

**Question.** Which fields give the sheet paper size and orientation, and each drawing view's placement, scale, orientation, and label?

**Known.** The sheet record stores a counted label, six property references, `controls:bytes[24]`, and the view-reference lists. The drawing view stores nineteen ordered operand references, `word:u16`, `values:u32[2]`, counted text, and an opaque tail.

**Need.** Each of these values requires its field, unit, and coordinate frame.

### DI-03. Cached drawing-view geometry

**Question.** Which sheet-cache records store the 2D curves of a drawing view, and what are their curve forms and coordinate frame?

**Known.** The cached-view record selects one drawing-view record through metadata sections 10, 8, and 7. The other sheet-cache and sheet-display classes are untyped.

**Need.** Curve records require exact layouts, their owning view, and their frame relative to the sheet.

### DI-04. Drawing dimensions

**Question.** Which records store drawing dimensions, and how do they encode the dimension kind, value, text, and attachment geometry?

**Known.** No dimension record class is typed.

**Need.** Dimension records require exact layouts and joins to the views and curves they annotate.

### DI-05. Title block and border

**Question.** Which records store the sheet border, the title block, and their text?

**Known.** The six sheet property references select one fixed class. Its role is not established.

**Need.** Border and title-block records require exact layouts and their text fields.

### DI-06. Drawing external-reference words

**Question.** How do the nonzero high words in metadata section 10 select their reference address space, and how do the section-9 forms contribute to revision counts?

**Known.** Section 9 has a u16 key, a u8 tag, and a tag-sized payload. Tags 1 and 2 have sixteen-byte payloads. Tag 3 has an eight-byte payload. Section 10 has eight-byte entries. A cached-view reference uses the zero-high-word, zero-key-extension form to select one drawing-view definition.

**Need.** Each additional form requires an exact target and count rule.

### DI-07. Drawing UFRx reference suffix

**Question.** What is the schema-12 drawing suffix grammar, and which identities join its document references to the RSe view definitions?

**Known.** The `UFRxDoc` prefix supplies schema and section versions, counted document text, and representation fields. The drawing view stores counted document-location text. The sheet supplies the main and alternate view-reference lists.

**Need.** The suffix fields require exact boundaries, reference encodings, and identity joins.

## 7. Evidence

### EV-01. Real-carrier golden fixtures

**Question.** Which committed golden input exercises real-carrier byte layouts?

**Known.** The Inventor goldens check two synthetic inputs, `structural` and `primary`, over the inspect and decode branches. The repository holds no `.ipt` or `.iam` fixture. The recorded Inventor evaluation covers 142 real files, whose bytes stay with the evaluator.

**Need.** Golden coverage stays limited to constructed declarations until a committed fixture carries observed real-carrier layouts. Real-carrier behavior needs such a fixture to prove it.
